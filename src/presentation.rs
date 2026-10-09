use serde_json::{Value,json};
use crate::{s,arr};
#[derive(Clone,Debug,PartialEq)]
pub enum Tone{Normal,Heading,Muted,Warning,Choice}
#[derive(Clone,Debug)]
pub struct Paragraph{pub text:String,pub tone:Tone}
fn judgments(result:&Value)->Vec<Value>{arr(&result["judgments"]).iter().map(|j|json!({"text":format!("{}\n{}{}",s(&j["choice"]),s(&j["reason"]),if s(&j["quote"]).is_empty(){String::new()}else{format!("\nRecorded statement: “{}”",s(&j["quote"]))}),"basis":j["status"],"evidence_ids":j["evidence_ids"]})).collect()}
/// Stable numbered references, including focused answers.
pub fn citations(artifact:&Value)->Vec<Value>{
    let result=&artifact["explanation"];let mut claims=judgments(result);
    if artifact["packet"]["focus_target"].is_null(){claims.push(result["answer"].clone());}
    claims.push(result["problem"].clone());
    if artifact["packet"]["focus_target"].is_null(){claims.extend([result["before"].clone(),result["after"].clone()]);}
    for key in ["steps","tradeoffs","checks"]{claims.extend(arr(&result[key]).iter().cloned());}
    claims.extend([result["answer"].clone(),result["before"].clone(),result["after"].clone()]);
    let mut cited=vec![];
    for claim in claims{for id in arr(&claim["evidence_ids"]){if !cited.iter().any(|e:&Value|e["id"]==*id){if let Some(e)=arr(&artifact["packet"]["evidence"]).iter().find(|e|e["id"]==*id){cited.push(e.clone());}}}}cited
}
pub fn reasoning(artifact:&Value)->Vec<Paragraph>{
    let mut checked=artifact.clone();crate::history::provenance::sanitize_artifact(&mut checked);let artifact=&checked;
    let result=&artifact["explanation"];let cited=citations(artifact);let mut out=vec![Paragraph{text:s(&result["title"]).into(),tone:Tone::Heading},Paragraph{text:"Based on captured evidence".into(),tone:Tone::Muted}];
    if let Some(warning)=artifact["provenance_warning"].as_str(){out.push(Paragraph{text:warning.into(),tone:Tone::Warning});}
    if !arr(&artifact["stale_files"]).is_empty()||artifact["stale_head"]==true{out.push(Paragraph{text:"EARLIER SNAPSHOT · files or Git HEAD changed during generation. Explain again for the latest state.".into(),tone:Tone::Warning});}
    let js=judgments(result);let mut reasons=if js.is_empty(){vec![result["problem"].clone()]}else{js};reasons.extend(arr(&result["steps"]).iter().cloned());reasons.extend(arr(&result["tradeoffs"]).iter().cloned());
    for(title,claims)in [("The change",vec![result["answer"].clone()]),("The reasoning",reasons),("What to check",arr(&result["checks"]).to_vec())]{
        if claims.is_empty(){continue;}out.push(Paragraph{text:title.into(),tone:Tone::Heading});
        for claim in claims{
            let refs=arr(&claim["evidence_ids"]).iter().filter_map(|id|cited.iter().position(|e|e["id"]==*id)).map(|i|format!(" [{}]",i+1)).collect::<String>();
            let label=match s(&claim["basis"]){"observed"=>"Evidence","assessment"=>"Interpretation","proposed"=>"Suggestion","unknown"=>"Not known","recorded"=>"Stated reason","inferred"=>"Inferred reason",_=>""};
            out.push(Paragraph{text:format!("{}{refs}  ({label})",s(&claim["text"])),tone:if ["recorded","inferred","unknown"].contains(&s(&claim["basis"])){Tone::Choice}else{Tone::Normal}});
        }
    }
    if !arr(&result["unknowns"]).is_empty(){out.push(Paragraph{text:"Still unknown".into(),tone:Tone::Warning});for item in arr(&result["unknowns"]){let mut text=s(item).to_owned();for e in arr(&artifact["packet"]["evidence"]){text=text.replace(s(&e["id"]),s(&e["file"]));}out.push(Paragraph{text,tone:Tone::Normal});}}
    out
}
pub fn reasoning_text(artifact:&Value)->String{reasoning(artifact).iter().map(|p|p.text.as_str()).collect::<Vec<_>>().join("\n\n")}
