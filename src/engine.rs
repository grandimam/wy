use serde_json::{Value,json};
use regex::Regex;
use std::collections::HashSet;
use crate::{arr,s,n,repository::{self,Change,Texts},security::digest,source};
pub fn code_evidence(file:&str,text:&str,line:usize,hashes:&Texts)->Value{
    let start=line.saturating_sub(2).max(1);let end=(line+2).min(text.lines().count()).max(start);
    json!({"id":format!("code-{}",&digest(&format!("{file}:{start}:{end}"))[..12]),"kind":"code","file":file,"symbol":"<module>","start_line":start,"end_line":end,"excerpt":source::excerpt(text,start,end),"snapshot_hash":hashes.get(file).cloned().unwrap_or_default(),"event_id":null,"session_id":null,"agent":null})
}
fn retrieve(file:&str,line:usize,token:&str,texts:&Texts,hashes:&Texts,sessions:&[Value])->Vec<Value>{
    let mut evidence=vec![code_evidence(file,&texts[file],line,hashes)];
    let patterns=vec![regex::escape(token)];
    let pattern=Regex::new(&format!("(?i){}",patterns.join("|"))).unwrap();
    'files:for(other,text) in texts{
        if other.ends_with(".md")||other.ends_with(".txt"){continue;}
        for (i,line) in text.lines().enumerate(){
            if pattern.is_match(line)&&!["#","//","from ","import "].iter().any(|p|line.trim_start().starts_with(p)){
                let e=code_evidence(other,text,i+1,hashes);if !evidence.iter().any(|x|x["id"]==e["id"]){evidence.push(e);}
                if evidence.len()>=7{break 'files;}
            }
        }
    }
    let mut candidates=vec![];
    for session in sessions{
        let matches:Vec<_>=arr(&session["events"]).iter().filter(|e|s(&e["text"]).to_lowercase().contains(&token.to_lowercase())&&s(&e["text"]).contains(file.rsplit('/').next().unwrap_or(file))).take(3).map(|e|json!({
            "id":format!("session-{}-{}-{}",s(&session["agent"]),&digest(&format!("{}{}",s(&session["id"]),s(&session["path"])))[..8],s(&e["id"])),"kind":"session","file":session["path"],"symbol":"<module>","start_line":e["source_line"],"end_line":e["source_line"],"excerpt":e["text"],"snapshot_hash":"","event_id":e["id"],"session_id":session["id"],"agent":session["agent"]})).collect();
        candidates.push(matches);
    }
    for i in 0..3 {for matches in &candidates{if let Some(item)=matches.get(i).filter(|_|evidence.len()<12){evidence.push(item.clone());}}}evidence
}
fn recorded(evidence:&[Value],sessions:&[Value],token:&str,file:&str)->Option<Value>{
    let start=Regex::new(r"^I (?:chose|used|introduced|added|selected|replaced)\b").unwrap();
    let cause=Regex::new(r"\b(?:because|so that|in order to)\b").unwrap();
    let negative=Regex::new(r"(?i)\b(?:not|never|didn't|don't|example|hypothetical)\b").unwrap();
    let sentences=Regex::new(r"[.!?]\s+|\n").unwrap();
    for item in evidence{
        if item["kind"]!="session"||!sessions.iter().any(|session|session["path"]==item["file"]&&arr(&session["events"]).iter().any(|e|e["id"]==item["event_id"]&&e["kind"]=="assistant")){continue;}
        for sentence in sentences.split(s(&item["excerpt"])){
            if start.is_match(sentence.trim())&&cause.is_match(sentence)&&sentence.to_lowercase().contains(&token.to_lowercase())&&sentence.contains(file)&&!negative.is_match(sentence){let mut item=item.clone();item["excerpt"]=json!(sentence);return Some(item);}
        }
    }None
}
pub fn analyze(changes:&[Change],texts:&Texts,hashes:&Texts,sessions:&[Value],baseline:bool)->Vec<Value>{
    let rules:Vec<Value>=serde_json::from_str(include_str!("data/rules.json")).unwrap();let mut decisions=vec![];let mut seen=HashSet::new();
    let dependency=Regex::new(r#"["\w][\w.-]+\s*(?:[<>=~^]+\s*\d|["']\s*:\s*["'][~^]?\d|=\s*["']\d)"#).unwrap();
    let config=Regex::new(r"\b(?:timeout|retries|max_workers|pool_size|replicas|concurrency)\s*[:=]").unwrap();
    for change in changes{
        let Some(text)=texts.get(&change.file) else{continue};let mut candidates=vec![];
        for(&line,code) in &change.additions{
            if ["#","//","import ","from "].iter().any(|p|code.trim_start().starts_with(p))||change.removed.iter().any(|old|old.trim()==code.trim()){continue;}
            for rule in &rules{
                let pattern=format!("{}{}",if rule["category"]=="database"{"(?i)"}else{""},s(&rule["pattern"]));
                if !Regex::new(&pattern).unwrap().is_match(code){continue;}
                let mut token=s(&rule["token"]).to_owned();if token=="abstraction"{if let Some(m)=Regex::new(r"class\s+(\w+)").unwrap().captures(code){token=m[1].into();}}
                candidates.push((line,rule.clone(),token));
            }
        }
        let name=change.file.rsplit('/').next().unwrap_or("");
        let dep=["package.json","Cargo.toml","go.mod"].contains(&name);
        let conf=[".yaml",".yml",".ini",".cfg"].iter().any(|x|name.ends_with(x))||["config.toml","settings.toml"].contains(&name);
        if let Some((&line,_))=change.additions.iter().find(|(_,s)|dep&&dependency.is_match(s)||conf&&config.is_match(s)){
            let r=if dep{json!({"category":"dependency","question":"Why change these dependencies?","alternatives":["Use existing dependencies","Implement a smaller local helper"],"gap":"Which capability and compatibility constraints justify these versions?"})}else{json!({"category":"configuration","question":"Why choose these operational limits?","alternatives":["Use the existing defaults","Derive limits from measured capacity"],"gap":"What measurements support these limits?"})};let token=s(&r["category"]).into();candidates.push((line,r,token));
        }
        for(line,rule,token) in candidates{
            let mut loc=repository::location(&change.file,text,line);let key=format!("{}:{}:{}",change.file,s(&loc["symbol"]),s(&rule["question"]));if !seen.insert(key.clone()){continue;}
            let mut evidence=retrieve(&change.file,line,&token,texts,hashes,sessions);let mut provenance="unexplained";let mut explanation="The change establishes this choice, but the available evidence does not establish why it was selected.".to_owned();let mut assumptions=vec![];let mut gaps=vec![rule["gap"].clone()];
            if let Some(statement)=recorded(&evidence,sessions,&token,&change.file){
                provenance="recorded";explanation=format!("The assistant explicitly stated: “{}”",s(&statement["excerpt"]));
                for e in &mut evidence{if e["id"]==statement["id"]{*e=statement.clone();}}
                assumptions.push(json!("This is an observable assistant statement, not proof that its justification is correct."));
            }
            let peers:std::collections::BTreeSet<_>=evidence.iter().filter(|e|e["kind"]=="code"&&e["file"]!=change.file&&s(&e["excerpt"]).contains(&token)).map(|e|s(&e["file"])).collect();
            if !peers.is_empty(){gaps.push(json!(format!("Similar code exists in {}; should this change follow the same convention?",peers.into_iter().collect::<Vec<_>>().join(", "))));}
            loc["start_line"]=json!(line);loc["end_line"]=json!(n(&loc["end_line"]).min(line+3));
            decisions.push(json!({"id":format!("decision-{}",&digest(&format!("{key}:{line}"))[..12]),"question":rule["question"],"category":rule["category"],"location":loc,"explanation":explanation,"provenance":provenance,"evidence":evidence,"alternatives":rule["alternatives"],"assumptions":assumptions,"unresolved_questions":gaps,"snapshot_hash":hashes.get(&change.file),"attribution":if baseline{"since-baseline"}else{"unknown"},"stale":false,"reflections":[]}));
            if decisions.len()>=12{return decisions;}
        }
    }decisions
}
