use anyhow::{Result,ensure,bail,Context};
use serde_json::{Value,json};
use similar::{TextDiff,ChangeTag};
use std::{collections::{BTreeMap,BTreeSet},path::{Path,PathBuf},process::Command};
use crate::{security::{self,redact,digest},source};
pub type Texts=BTreeMap<String,String>;
pub struct Sources {pub texts:Texts,pub hashes:Texts,pub warnings:Vec<String>}
#[derive(Clone,Debug,Default)]
pub struct Change {pub file:String,pub additions:BTreeMap<usize,String>,pub removed:Vec<String>,pub hunks:Vec<(usize,usize)>,pub patch:String}
pub fn git(root:&Path,args:&[&str],check:bool)->Result<String>{
    let out=Command::new("git").args(["--no-pager","-c","core.quotePath=false","-c","core.fsmonitor=false"]).args(args).current_dir(root).output().context("Could not run Git")?;
    if check&&!out.status.success(){bail!("Git operation failed: {}",redact(&String::from_utf8_lossy(&out.stderr)).trim());}
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
pub fn root(path:&Path)->Result<PathBuf>{Ok(Path::new(git(path,&["rev-parse","--show-toplevel"],true)?.trim()).canonicalize()?)}
pub fn head(root:&Path)->Option<String>{git(root,&["rev-parse","--verify","HEAD"],false).ok().map(|s|s.trim().to_owned()).filter(|s|!s.is_empty())}
pub fn resolve_commit(root:&Path,revision:&str)->Result<String>{
    Ok(git(root,&["rev-parse","--verify","--end-of-options",&format!("{revision}^{{commit}}")],true)?.trim().to_owned())
}
pub fn sources(root:&Path)->Result<Sources>{
    let names=git(root,&["ls-files","-z","--cached","--others","--exclude-standard"],true)?;
    let mut sources=Sources{texts:Texts::new(),hashes:Texts::new(),warnings:vec![]};let mut total=0;
    for name in names.split('\0').filter(|n|security::allowed(n)).collect::<BTreeSet<_>>(){
        if let Some(text)=security::read_source(root,name){
            total+=text.len();if total>security::MAX_REPO{sources.warnings.push("Repository context truncated at 12 MB".into());break;}
            sources.hashes.insert(name.into(),digest(&text));sources.texts.insert(name.into(),redact(&text));
        }else{sources.warnings.push(format!("Skipped unreadable, binary, symlinked or oversized file: {name}"));}
    }Ok(sources)
}
pub fn committed(root:&Path,revision:&str)->Result<Texts>{
    Ok(committed_sources(root,revision)?.texts)
}
pub fn committed_sources(root:&Path,revision:&str)->Result<Sources>{
    let commit=resolve_commit(root,revision)?;
    let records=git(root,&["ls-tree","-r","-z","-l",commit.trim()],true)?;
    let mut sources=Sources{texts:Texts::new(),hashes:Texts::new(),warnings:vec![]};let mut total=0;
    for record in records.split('\0'){
        let Some((meta,name))=record.split_once('\t') else{continue};let parts:Vec<_>=meta.split_whitespace().collect();
        if parts.len()!=4 || parts[0]=="120000" || parts[1]!="blob" || !security::allowed(name){continue;}
        let size=parts[3].parse::<usize>().unwrap_or(usize::MAX);if size>security::MAX_FILE as usize{continue;}
        total+=size;ensure!(total<=security::MAX_REPO,"Base revision exceeds the 12 MB source limit");
        let text=git(root,&["cat-file","blob",parts[2]],true)?;
        if !text.contains('\0'){sources.hashes.insert(name.into(),digest(&text));sources.texts.insert(name.into(),redact(&text));}
    }Ok(sources)
}
pub fn compare(before:&Texts,after:&Texts)->Vec<Change>{
    let mut result=Vec::new();
    for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>(){
        let old=before.get(name).map(String::as_str).unwrap_or("");let new=after.get(name).map(String::as_str).unwrap_or("");
        if old==new{continue;}
        let diff=TextDiff::from_lines(old,new);let mut c=Change{file:name.clone(),..Default::default()};let mut at=1;
        for change in diff.iter_all_changes(){match change.tag(){
            ChangeTag::Equal=>at=change.new_index().unwrap_or(0)+2,
            ChangeTag::Insert=>{let n=change.new_index().unwrap_or(0)+1;c.additions.insert(n,change.value().trim_end_matches(['\r','\n']).into());c.hunks.push((n,n));at=n+1;},
            ChangeTag::Delete=>{c.removed.push(change.value().trim_end_matches(['\r','\n']).into());c.hunks.push((at,at));}
        }}
        c.hunks.dedup();c.patch=diff.unified_diff().context_radius(4).header(&format!("a/{name}"),&format!("b/{name}")).to_string();result.push(c);
    }result
}
pub fn location(file:&str,text:&str,line:usize)->Value{
    let found=source::outline(file,text).into_iter().filter(|s|s.start<=line&&line<=s.end).min_by_key(|s|s.end-s.start);
    if let Some(s)=found{json!({"file":file,"symbol":s.name,"start_line":s.start,"end_line":s.end})}else{json!({"file":file,"symbol":"<module>","start_line":line,"end_line":line})}
}
pub fn changed_file(c:&Change,text:&str)->Value{
    let lines:BTreeSet<_>=c.additions.keys().copied().chain(c.hunks.iter().map(|h|h.0)).collect();
    let symbols:Vec<_>=source::outline(&c.file,text).iter().filter(|s|lines.iter().any(|n|s.start<=*n&&*n<=s.end)).map(|s|json!({"file":c.file,"symbol":s.name,"start_line":s.start,"end_line":s.end})).collect();
    let patch=redact(&c.patch);
    json!({"file":c.file,"hunks":c.hunks.iter().map(|(a,b)|json!({"file":c.file,"symbol":"<module>","start_line":a,"end_line":b})).collect::<Vec<_>>(),"symbols":symbols,"added_lines":c.additions.keys().collect::<Vec<_>>(),"removed_line_count":c.removed.len(),"diff":security::short(&patch,40000),"diff_truncated":patch.chars().count()>40000})
}
