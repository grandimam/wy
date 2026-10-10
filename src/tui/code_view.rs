//! Render code as code, rather than exposing diff transport metadata as prose.
use super::{document::Document,theme::*};
use ratatui::prelude::*;

fn start(header:&str,sign:char)->Option<usize>{
    header.split_whitespace().find_map(|field|field.strip_prefix(sign).and_then(|n|n.split(',').next()).and_then(|n|n.parse().ok()))
}
fn number(n:Option<usize>)->String{n.filter(|n|*n>0).map(|n|n.to_string()).unwrap_or_default()}
fn bump(n:&mut Option<usize>){if let Some(n)=n{*n+=1;}}

pub(super) fn render(doc:&mut Document,text:&str,format:&str,historical:bool,limit:usize){
    render_code(doc,text,format,historical,limit);
    technical_details(doc,text,format,vec![]);
}

pub(super) fn technical_details(doc:&mut Document,text:&str,format:&str,mut details:Vec<Line<'static>>){
    details.push(Line::styled(if format=="patch"{"Raw patch · diagnostic view"}else{"Raw recorded content · diagnostic view"},Style::default().fg(TEXT).bold()));
    details.push(Line::styled("Underlying captured text, including transport markers.",Style::default().fg(TEXT)));
    details.extend(text.lines().map(|line|Line::styled(line.to_owned(),Style::default().fg(TEXT))));
    doc.disclosure(format!("raw-code-{}",doc.lines.len()),"Technical details",details);
}

pub(super) fn render_code(doc:&mut Document,text:&str,format:&str,historical:bool,limit:usize){
    let block_start=doc.lines.len();
    if historical {
        doc.text(if format=="patch"{"Replacement excerpts · file line numbers unavailable"}else{"Recorded content · may differ from the current file"},TEXT);
    } else {doc.text("Current Git diff · old / new line numbers",TEXT);}
    doc.text("Code does not wrap · ←/→ or h/l scroll horizontally",TEXT);
    let raw:Vec<_>=text.lines().collect();
    if format=="code"{
        doc.heading("RECORDED CONTENT");
        for (i,line) in raw.iter().take(limit).enumerate(){doc.code_line(format!("{:>5} │ ",i+1),line,TEXT);}
        if raw.len()>limit{doc.text(format!("Showing {limit} of {} excerpt lines. Open the conversation for the complete captured edit.",raw.len()),TEXT);}
    }else if historical{
        let mut chunks:Vec<Vec<&str>>=vec![];let mut in_hunk=false;
        for line in &raw{
            if line.starts_with("@@") {in_hunk=true;chunks.push(vec![]);continue;}
            if (!in_hunk&&(line.starts_with("--- ")||line.starts_with("+++ ")))||line.starts_with("\\ No newline"){continue;}
            if chunks.is_empty(){chunks.push(vec![]);}
            chunks.last_mut().unwrap().push(line);
        }
        let mut shown=0;
        for (i,chunk) in chunks.iter().filter(|c|!c.is_empty()).enumerate(){
            if shown>=limit{doc.text("More replacement excerpts are available in Technical details.",TEXT);break;}
            doc.heading(format!("REPLACEMENT {}",i+1));
            for (title,sign,color) in [("BEFORE",'-',RED),("AFTER",'+',GREEN)]{
                doc.text(title,color);
                let lines:Vec<_>=chunk.iter().filter_map(|line|line.strip_prefix(sign).or_else(||line.strip_prefix(' '))).collect();
                if lines.is_empty(){doc.code_line("      │ ".into(),"(empty)",TEXT);}
                for line in lines.iter().take(limit.saturating_sub(shown)){doc.code_line(format!("    {sign} │ "),line,color);shown+=1;}
                if shown>=limit && !lines.is_empty(){doc.text("Excerpt display limit reached; Technical details retains the captured text.",TEXT);}
            }
        }
    }else{
        let mut old=None;let mut new=None;let mut shown=0;let mut in_hunk=false;
        doc.text("  OLD   NEW   │",TEXT);
        for line in &raw {
            if line.starts_with("@@") {in_hunk=true;old=start(line,'-');new=start(line,'+');continue;}
            if (!in_hunk&&(line.starts_with("--- ")||line.starts_with("+++ ")||line.starts_with("diff --git")||line.starts_with("index ")))||line.starts_with("\\ No newline"){continue;}
            let (before,after,sign,body,color)=if let Some(body)=line.strip_prefix('+'){
                let n=new;bump(&mut new);(None,n,'+',body,GREEN)
            }else if let Some(body)=line.strip_prefix('-'){
                let n=old;bump(&mut old);(n,None,'-',body,RED)
            }else if let Some(body)=line.strip_prefix(' '){
                let (a,b)=(old,new);bump(&mut old);bump(&mut new);(a,b,' ',body,TEXT)
            }else{continue};
            if shown>=limit{doc.text("More lines are available in Technical details.",TEXT);break;}
            doc.code_line(format!("{:>5} {:>5} {sign} │ ",number(before),number(after)),body,color);shown+=1;
        }
    }
    doc.gap();
    let title=if historical&&format=="code"{format!("Code saved in this session · {} lines",raw.len())}else if historical{"Before / after · saved replacements".into()}else{"Code changes".into()};
    doc.fold_code(format!("code-block-{block_start}"),&title,block_start,!historical);
}
