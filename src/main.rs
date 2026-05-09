use std::path::PathBuf;

use clap::Parser;

use postui::{
    app, cli::Cli, config::Config, db::PgConn, error::{ConfigError, Result}, logging, term,
};

fn default_config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "postui")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();
    let cli = Cli::parse();

    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        tracing::info!(path = %config_path.display(), "no config file; using defaults");
        Config::default()
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let conn = bootstrap_connection(&cli, &config).await?;
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let mut app = app::App::new(config.clone(), config_path);
        if let Some(c) = conn {
            app.set_connection(c);
        } else if !config.connections.is_empty() {
            use postui::views::connections::ConnectionsView;
            app.push_view(Box::new(ConnectionsView::new(&config, None)));
        }
        app.run(&mut term).await
    })
}

async fn bootstrap_connection(cli: &Cli, config: &Config) -> Result<Option<PgConn>> {
    if let Some(uri) = &cli.uri {
        let label = label_for_uri(uri);
        return Ok(Some(PgConn::connect(uri, label).await?));
    }
    if let Some(name) = &cli.connection {
        let cfg = config.find_connection(name).ok_or_else(|| {
            ConfigError::Parse(format!("no connection named '{name}' in config"))
        })?;
        let resolved = cfg.resolve_secrets()?;
        let target = resolved.as_target()?;
        return Ok(Some(PgConn::connect(&target, name.clone()).await?));
    }
    Ok(None)
}

fn label_for_uri(uri: &str) -> String {
    if let Ok(u) = url::Url::parse(uri) {
        let user = u.username();
        let host = u.host_str().unwrap_or("");
        let db = u.path().trim_start_matches('/');
        format!("{user}@{host}/{db}")
    } else {
        "uri".into()
    }
}
