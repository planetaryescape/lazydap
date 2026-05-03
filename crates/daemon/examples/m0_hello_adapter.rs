use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut child = Command::new("codelldb")
        .arg("--port")
        .arg("0")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stderr = child.stderr.take().expect("stderr is piped");
    let mut buf = [0u8; 256];
    let n = stderr.read(&mut buf).await?;
    let s = std::str::from_utf8(&buf[..n])?;
    println!("first stderr chunk: {s:?}");

    child.kill().await?;
    Ok(())
}
