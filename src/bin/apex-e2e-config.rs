use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "apex-e2e-config")]
struct Args {
    #[arg(long, default_value = ".env.e2e")]
    env_file: PathBuf,
    #[arg(long, default_value = ".run/e2e/generated.e2e.config.json")]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let env = apex::e2e::E2eEnv::from_env_file(&args.env_file)
        .with_context(|| format!("failed to parse {}", args.env_file.display()))?;
    let config = apex::e2e::write_config(&env, &args.output)
        .with_context(|| format!("failed to write {}", args.output.display()))?;

    println!(
        "wrote {} with {} channel(s), router {}, team {}",
        args.output.display(),
        config.channels.len(),
        env.router_name,
        env.team_id
    );

    Ok(())
}
