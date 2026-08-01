use lazydap_dap::{Capabilities, DapTransport, InitializeArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Through the adapter module rather than with a hand-written recipe: how
    // codelldb is started — its flags, its `RUST_LOG`, which stream it
    // announces its port on — is a fact about codelldb, and a second copy of
    // it here would be one that can drift.
    let lazydap_daemon::adapter::Spawn::Tcp(spawn) =
        lazydap_daemon::adapter::for_kind(lazydap_core::AdapterKind::Codelldb)
            .spawn(std::path::Path::new("codelldb"))
    else {
        unreachable!("codelldb speaks DAP over TCP")
    };
    let mut transport = DapTransport::spawn_tcp(&spawn).await?;
    let initialize_args: InitializeArgs = InitializeArgs {
        client_id: Some(String::from("lazydap")),
        client_name: Some(String::from("lazydap")),
        adapter_id: Some(String::from("lldb")),
        lines_start_at1: true,
        columns_start_at1: true,
        supports_variable_type: true,
        path_format: Some(String::from("path")),
        locale: Some(String::from("en-US")),
    };

    let caps: Capabilities = transport.request("initialize", &initialize_args).await?;

    println!("{caps:#?}");
    Ok(())
}
