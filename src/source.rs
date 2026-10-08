//! Syntax navigation via tree-sitter; never imports or runs repository code.
use serde::{Serialize,Deserialize};
use tree_sitter::{Language,Node,Parser,Tree};
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Symbol {pub name:String,pub kind:String,pub start:usize,pub end:usize,pub depth:usize}
pub fn parse(file:&str,text:&str)->Option<Tree>{
    let language:Language=match file.rsplit('.').next()? {
        "rs"=>tree_sitter_rust::LANGUAGE.into(),
        "js"|"jsx"=>tree_sitter_javascript::LANGUAGE.into(),"ts"=>tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx"=>tree_sitter_typescript::LANGUAGE_TSX.into(),"json"=>tree_sitter_json::LANGUAGE.into(),_=>return None};
    let mut parser=Parser::new();parser.set_language(&language).ok()?;parser.parse(text,None)
}
pub fn outline(file:&str,text:&str)->Vec<Symbol>{
    if file.ends_with(".md") {
        let mut found:Vec<Symbol>=Vec::new();let mut fence=false;
        for (i,line) in text.lines().enumerate(){
            if line.trim_start().starts_with("```")||line.trim_start().starts_with("~~~"){fence=!fence;}
            let depth=line.chars().take_while(|c|*c=='#').count();
            if !fence && (1..=6).contains(&depth) && line.as_bytes().get(depth)==Some(&b' '){
                found.push(Symbol{name:line[depth+1..].to_owned(),kind:"heading".into(),start:i+1,end:text.lines().count(),depth:depth-1});
            }
        }
        for i in 0..found.len(){if let Some(next)=found[i+1..].iter().find(|s|s.depth<=found[i].depth){found[i].end=next.start-1;}}
        return found;
    }
    let Some(tree)=parse(file,text) else{return Vec::new()};
    let mut found=Vec::new();
    fn walk(node:Node,text:&str,parents:&[String],found:&mut Vec<Symbol>,depth:usize){
        if found.len()>=500 || depth>128{return;}
        let mut parents=parents.to_vec();
        let kind=match node.kind(){
            "class_declaration"=>Some("class"),
            "function_item"|"function_declaration"|"method_definition"=>Some("function"),
            "struct_item"=>Some("struct"),"enum_item"=>Some("enum"),"trait_item"=>Some("trait"),
            "impl_item"=>Some("impl"),"mod_item"=>Some("module"),"pair"=>Some("key"),_=>None};
        if let Some(kind)=kind {
            let name=node.child_by_field_name(if kind=="impl"{"type"}else if kind=="key"{"key"}else{"name"});
            if let Some(name)=name.and_then(|n|n.utf8_text(text.as_bytes()).ok()){
                parents.push(name.into());
                let start=node.start_position().row+1;
                found.push(Symbol{name:parents.join("."),kind:kind.into(),start,end:node.end_position().row+1,depth:parents.len()-1});
            }
        }
        let mut cursor=node.walk();for child in node.named_children(&mut cursor){walk(child,text,&parents,found,depth+1);}
    }
    walk(tree.root_node(),text,&[],&mut found,0);found
}
pub fn syntax_at(file:&str,text:&str,line:usize,kinds:&[&str])->bool{
    let Some(tree)=parse(file,text) else{return true};
    if tree.root_node().has_error(){return false;}
    let mut stack=vec![tree.root_node()];
    while let Some(node)=stack.pop(){
        if node.start_position().row+1==line && kinds.contains(&node.kind()){return true;}
        let mut cur=node.walk();stack.extend(node.named_children(&mut cur));
    }false
}
pub fn window(text:&str,symbols:&[Symbol],line:usize,limit:usize)->(usize,usize,String){
    let count=text.lines().count().max(1);let line=line.clamp(1,count);
    let symbol=symbols.iter().filter(|s|s.start<=line&&line<=s.end).min_by_key(|s|s.end-s.start);
    let(mut a,mut b)=(line.saturating_sub(6).max(1),(line+7).min(count));
    if let Some(s)=symbol.filter(|s|s.end-s.start<limit){a=a.min(s.start);b=b.max(s.end);}
    if b-a+1>limit{a=line.saturating_sub(8).max(1);b=(a+limit-1).min(count);}
    (a,b,excerpt(text,a,b))
}
pub fn excerpt(text:&str,start:usize,end:usize)->String{text.lines().skip(start.saturating_sub(1)).take(end.saturating_sub(start)+1).collect::<Vec<_>>().join("\n")}
