//! Opt-in agent process, isolated from repository tools and configuration.
use anyhow::{Result,ensure,bail,Context};
use serde_json::Value;
use std::{path::Path,process::{Command,Stdio,Child},sync::{Arc,atomic::{AtomicBool,Ordering}},time::{Instant,Duration},io::Write};
use crate::security;
pub type Cancel=Arc<AtomicBool>;
pub fn available(agent:&str)->bool{std::env::var_os("PATH").is_some_and(|p|std::env::split_paths(&p).any(|d|d.join(agent).is_file()))}
pub fn arguments(agent:&str,directory:&Path,schema:&Value)->Result<Vec<String>>{
    ensure!(["codex","claude"].contains(&agent),"Choose codex or claude for reasoning");
    if agent=="claude"{return Ok(["--print","--output-format","json","--json-schema",&schema.to_string(),"--tools","","--strict-mcp-config","--mcp-config",r#"{"mcpServers":{}}"#,"--setting-sources","","--settings",r#"{"disableAllHooks":true}"#,"--disable-slash-commands","--no-chrome","--no-session-persistence"].map(str::to_owned).to_vec());}
    let path=directory.join("response-schema.json");std::fs::write(&path,schema.to_string())?;
    let mut args=["exec","--ignore-user-config","--ignore-rules","--ephemeral","--sandbox","read-only","--skip-git-repo-check","--color","never","--output-schema"].map(str::to_owned).to_vec();
    args.extend([path.to_string_lossy().into_owned(),"--output-last-message".into(),directory.join("response.json").to_string_lossy().into_owned()]);
    for feature in ["shell_tool","unified_exec","multi_agent","hooks","apps","plugins","browser_use","computer_use","image_generation","skill_search"]{args.extend(["--disable".into(),feature.into()]);}
    args.extend(["--enable","skip_host_skill_discovery","--config",r#"web_search="disabled""#,"-"].map(str::to_owned));Ok(args)
}
struct Process(Child);
impl Drop for Process{fn drop(&mut self){
    // Also terminate descendant processes when the direct child exits early.
    #[cfg(unix)] unsafe{libc::kill(-(self.0.id() as i32),libc::SIGKILL);}
    let _=self.0.kill();let _=self.0.wait();
}}
pub fn invoke(agent:&str,prompt:&str,schema:&Value,cancel:&Cancel,timeout:Duration)->Result<Value>{
    ensure!(!cancel.load(Ordering::Relaxed),"Reasoning cancelled");
    ensure!(available(agent),"Install and sign in to the {agent} CLI before using it for reasoning");
    let directory=tempfile::Builder::new().prefix("wy-reason-").tempdir()?;
    let stdout=tempfile::tempfile()?;let stderr=tempfile::tempfile()?;
    let mut cmd=Command::new(agent);cmd.args(arguments(agent,directory.path(),schema)?).current_dir(directory.path()).env("NO_COLOR","1").stdin(Stdio::piped()).stdout(stdout.try_clone()?).stderr(stderr.try_clone()?);
    #[cfg(unix)] {use std::os::unix::process::CommandExt;cmd.process_group(0);}
    let mut process=Process(cmd.spawn().with_context(||format!("Could not start {agent}; check its installation"))?);
    let mut stdin=process.0.stdin.take().unwrap();let prompt=prompt.to_owned();
    let writer=std::thread::spawn(move||stdin.write_all(prompt.as_bytes()));let started=Instant::now();
    let status=loop{
        ensure!(!cancel.load(Ordering::Relaxed),"Reasoning cancelled");
        ensure!(started.elapsed()<timeout,"{agent} exceeded the {} second reasoning timeout",timeout.as_secs());
        ensure!(stdout.metadata()?.len()<=1_000_000&&stderr.metadata()?.len()<=1_000_000,"Agent response exceeded the output limit");
        if let Some(status)=process.0.try_wait()?{break status;}
        std::thread::sleep(Duration::from_millis(80));
    };
    ensure!(status.success(),"{agent} reasoning failed (exit {}); check CLI sign-in and connectivity",status.code().unwrap_or(-1));
    let _=writer.join();
    let mut result=if agent=="codex"{
        let path=directory.path().join("response.json");ensure!(path.is_file()&&path.metadata()?.len()<=200_000,"Codex did not return a bounded structured answer");
        serde_json::from_str::<Value>(&std::fs::read_to_string(path)?).context("Agent returned an invalid structured answer")?
    }else{
        use std::io::{Read,Seek,SeekFrom};let mut out=stdout;out.seek(SeekFrom::Start(0))?;let mut raw=String::new();out.take(1_000_001).read_to_string(&mut raw)?;ensure!(raw.len()<=1_000_000,"Claude response exceeded the output limit");
        let envelope:Value=serde_json::from_str(&raw).context("Agent returned an invalid structured answer")?;
        ensure!(envelope["is_error"]!=true,"Claude returned an error; check CLI sign-in and connectivity");
        if envelope["structured_output"].is_object(){envelope["structured_output"].clone()}else{serde_json::from_str(crate::s(&envelope["result"])).context("Agent returned an invalid structured answer")?}
    };
    if !result.is_object(){bail!("Agent did not return a structured object");}security::clean(&mut result);Ok(result)
}
