use lazydap_dap::{Capabilities, DapTransport, InitializeArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut transport = DapTransport::spawn("codelldb").await?;
    let initialize_args: InitializeArgs = InitializeArgs {
        client_id: Some(String::from("lazydap")),
        client_name: Some(String::from("lazydap")),
        adapter_id: Some(String::from("lldb")),
        lines_start_at1: true,
        columns_start_at1: true,
        path_format: Some(String::from("path")),
        locale: Some(String::from("en-US"))
    };

    let caps: Capabilities = transport.request("initialize", &initialize_args).await?;

    println!("{caps:#?}");
    Ok(())
}
