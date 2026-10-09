use anyhow::{Result,ensure,Context};
use serde_json::{Value,json};
use std::{io::Read,time::Duration};
use crate::{arr,s,security::{redact,short,clean}};
pub struct Provider {client:reqwest::blocking::Client,endpoint:String,model:String,pub input:u64,pub output:u64}
impl Provider{
    pub fn new(endpoint:&str,model:&str,allow_remote:bool)->Result<Self>{
        let url=reqwest::Url::parse(endpoint).context("Provider endpoint must be HTTP(S)")?;
        ensure!(url.username().is_empty()&&url.password().is_none()&&url.query().is_none()&&url.fragment().is_none(),"Provider URL cannot contain credentials, query parameters or fragments");
        ensure!(["http","https"].contains(&url.scheme())&&url.host_str().is_some(),"Provider endpoint must be HTTP(S)");
        let local=["localhost","127.0.0.1","[::1]","::1"].contains(&url.host_str().unwrap_or(""));
        ensure!(local||allow_remote&&url.scheme()=="https","Remote providers require HTTPS and WY_ALLOW_REMOTE=1; source excerpts will be sent");
        ensure!(!model.is_empty(),"Set WY_MODEL to an explicitly selected model");
        let client=reqwest::blocking::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(60)).build()?;
        Ok(Self{client,endpoint:endpoint.into(),model:model.into(),input:0,output:0})
    }
    pub fn from_env()->Result<Self>{Self::new(&std::env::var("WY_MODEL_ENDPOINT").unwrap_or("http://127.0.0.1:11434/api/chat".into()),&std::env::var("WY_MODEL").unwrap_or_default(),std::env::var("WY_ALLOW_REMOTE").as_deref()==Ok("1"))}
    fn request(&mut self,payload:Value,schema:&str)->Result<Value>{
        let body=json!({"model":self.model,"stream":false,"format":crate::schema(schema),"messages":[{"role":"system","content":crate::reasoning::prompt("provider")},{"role":"user","content":redact(&payload.to_string())}],"options":{"temperature":0,"num_predict":1800}});
        let mut request=self.client.post(&self.endpoint).json(&body);
        if let Ok(key)=std::env::var("WY_PROVIDER_API_KEY"){request=request.bearer_auth(key);}
        let response=request.send().map_err(|_|anyhow::anyhow!("Model provider request failed; check endpoint, service and credentials"))?;
        ensure!(!response.status().is_redirection(),"Provider redirects are disabled");
        ensure!(response.status().is_success(),"Model provider request failed; check endpoint, service and credentials");
        let mut bytes=vec![];response.take(1_000_001).read_to_end(&mut bytes)?;ensure!(bytes.len()<=1_000_000,"Provider response exceeds 1 MB");
        let envelope:Value=serde_json::from_slice(&bytes).context("Model provider returned an invalid structured response")?;
        self.input+=envelope["prompt_eval_count"].as_u64().unwrap_or(0);self.output+=envelope["eval_count"].as_u64().unwrap_or(0);
        let mut result:Value=serde_json::from_str(s(&envelope["message"]["content"])).context("Model provider returned an invalid structured response")?;clean(&mut result);crate::validate(schema,&result)?;Ok(result)
    }
    pub fn ask(&mut self,decision:&Value,question:&str)->Result<Value>{
        let result=self.request(json!({"task":"investigate","question":short(question,4000),"decision":decision}),"Answer")?;
        validate_citations(&result["evidence_ids"],decision)?;ensure!(!arr(&result["evidence_ids"]).is_empty(),"Model answer has no supporting citations");Ok(result)
    }
    pub fn enrich(&mut self,decision:&mut Value)->Result<()>{
        if decision["provenance"]=="recorded"{return Ok(());}
        let result=self.request(json!({"task":"justify","decision":decision}),"Justification")?;
        validate_citations(&result["evidence_ids"],decision)?;
        ensure!(!crate::history::provenance::secondary_only(arr(&decision["evidence"]))||result["provenance"]=="unexplained","Original turn unavailable; secondary evidence cannot establish inferred original intent");
        ensure!(result["provenance"]!="inferred"||!arr(&result["evidence_ids"]).is_empty(),"Model inference has no supporting citations");
        for key in ["explanation","provenance","alternatives"]{decision[key]=result[key].clone();}
        decision["assumptions"]=json!([vec![json!("Model-generated hypothesis; citation existence is checked, semantic entailment needs review.")],arr(&result["assumptions"]).to_vec()].concat());
        for q in arr(&result["unresolved_questions"]){if !arr(&decision["unresolved_questions"]).contains(q){decision["unresolved_questions"].as_array_mut().unwrap().push(q.clone());}}
        if !arr(&result["evidence_ids"]).is_empty(){decision["evidence"]=json!(arr(&decision["evidence"]).iter().filter(|e|arr(&result["evidence_ids"]).contains(&e["id"])).cloned().collect::<Vec<_>>());}Ok(())
    }
}
pub fn validate_citations(ids:&Value,decision:&Value)->Result<()>{
    ensure!(arr(ids).iter().all(|id|arr(&decision["evidence"]).iter().any(|e|e["id"]==*id)),"Model cited evidence that was not supplied");Ok(())
}
