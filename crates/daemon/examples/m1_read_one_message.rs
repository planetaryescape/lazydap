use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut child, port) = spawn_codelldb_and_get_port().await?;
    println!("codelldb listening on port {port}");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;

    let request = serde_json::json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "lazydap",
            "clientName": "lazydap",
            "adapterID": "lldb",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
        },
    });
    let request_bytes = serde_json::to_vec(&request)?;
    let header = format!("Content-Length: {}\r\n\r\n", request_bytes.len());

    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;

    let response = read_one_message(&mut stream).await?;

    println!("---- DAP response ----");
    println!("{}", serde_json::to_string_pretty(&response)?);

    child.kill().await?;
    Ok(())
}

async fn read_one_message(stream: &mut TcpStream) -> anyhow::Result<serde_json::Value> {
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("EOF before end of headers");
        }
        let line = buf.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse()?);
        }
    }

    let content_length =
        content_length.ok_or_else(|| anyhow::anyhow!("no Content-Length header"))?;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;

    let value: serde_json::Value = serde_json::from_slice(&body)?;
    Ok(value)
}

async fn spawn_codelldb_and_get_port() -> anyhow::Result<(tokio::process::Child, u16)> {
    let mut child = Command::new("codelldb")
        .arg("--port")
        .arg("0")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().expect("stderr is piped");
    let mut lines = BufReader::new(stderr).lines();

    while let Some(line) = lines.next_line().await? {
        // codelldb's log format drifts across versions. Modern (20.x):
        // "Listening on HOST:PORT". Older: "Listening on port NNNNN".
        // See docs/issues/0002-codelldb-version-drift-rust-log.md.
        let Some((_, rest)) = line.split_once("Listening on ") else {
            continue;
        };
        let port_str = rest
            .strip_prefix("port ")
            .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
        let port: u16 = port_str.trim().parse()?;
        // Without a drain, codelldb's stderr pipe fills (~64 KiB kernel
        // buffer) and the adapter blocks on its next log write.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        return Ok((child, port));
    }
    anyhow::bail!("codelldb did not print a 'Listening on' line")
}

#[cfg(test)]
mod tests {
    use super::read_one_message;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn reads_a_content_length_framed_dap_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body = br#"{"type":"response","command":"initialize","success":true,"seq":1,"request_seq":1}"#;
            let header = format!("Content-Length: {}\r\n\r\n", body.len());
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
            stream.flush().await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let value = read_one_message(&mut client).await.unwrap();
        server.await.unwrap();

        assert_eq!(value["type"], "response");
        assert_eq!(value["command"], "initialize");
        assert_eq!(value["success"], true);
    }
}
