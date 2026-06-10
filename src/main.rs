use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use tson::{SchemaOptions, schema_from_source};

#[derive(Parser)]
#[command(name = "tson")]
#[command(about = "Generate JSON Schema from TypeScript classes and interfaces")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// TypeScript files to read. Used by the default generate command.
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Class or interface name to generate. Defaults to the first class/interface found.
    #[arg(short = 'c', long = "class", value_name = "NAME")]
    class_name: Option<String>,

    /// Write the schema to a file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,

    /// Allow properties not declared by the TypeScript type.
    #[arg(long)]
    allow_additional: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a JSON Schema file from TypeScript input.
    Generate(GenerateArgs),
}

#[derive(Parser)]
struct GenerateArgs {
    /// TypeScript files to read.
    #[arg(required = true, value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Class or interface name to generate. Defaults to the first class/interface found.
    #[arg(short = 'c', long = "class", value_name = "NAME")]
    class_name: Option<String>,

    /// Write the schema to a file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,

    /// Allow properties not declared by the TypeScript type.
    #[arg(long)]
    allow_additional: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let args = match cli.command {
        Some(Command::Generate(args)) => args,
        None => GenerateArgs {
            files: cli.files,
            class_name: cli.class_name,
            out: cli.out,
            allow_additional: cli.allow_additional,
        },
    };

    if args.files.is_empty() {
        return Err("at least one TypeScript file is required".into());
    }

    let mut source = String::new();
    for path in &args.files {
        source.push_str(&fs::read_to_string(path)?);
        source.push('\n');
    }

    let schema = schema_from_source(
        &source,
        SchemaOptions {
            root_type: args.class_name,
            additional_properties: args.allow_additional,
        },
    )?;
    let output = serde_json::to_string_pretty(&schema)?;

    match args.out {
        Some(path) => fs::write(path, format!("{output}\n"))?,
        None => println!("{output}"),
    }

    Ok(())
}
