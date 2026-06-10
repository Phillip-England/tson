use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use tson::{ExampleOptions, SchemaOptions, examples_from_schema, schema_from_source};

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

    /// Copy the schema JSON to the clipboard.
    #[arg(long)]
    clipboard: bool,

    /// Print the schema JSON to stdout, even when writing to a file or clipboard.
    #[arg(long)]
    print: bool,

    /// Allow properties not declared by the TypeScript type.
    #[arg(long)]
    allow_additional: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a JSON Schema file from TypeScript input.
    Generate(GenerateArgs),

    /// Generate valid or invalid JSON examples for a TypeScript type.
    Examples(ExamplesArgs),
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

    /// Copy the schema JSON to the clipboard.
    #[arg(long)]
    clipboard: bool,

    /// Print the schema JSON to stdout, even when writing to a file or clipboard.
    #[arg(long)]
    print: bool,

    /// Allow properties not declared by the TypeScript type.
    #[arg(long)]
    allow_additional: bool,
}

#[derive(Parser)]
struct ExamplesArgs {
    /// TypeScript files to read.
    #[arg(required = true, value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Class or interface name to generate examples for. Defaults to the first class/interface found.
    #[arg(short = 'c', long = "class", value_name = "NAME")]
    class_name: Option<String>,

    /// Number of examples to generate.
    #[arg(short = 'n', long, default_value_t = 3, value_name = "COUNT")]
    count: usize,

    /// Generate examples that should fail validation against the schema.
    #[arg(long, conflicts_with = "valid")]
    invalid: bool,

    /// Generate examples that should pass validation against the schema.
    #[arg(long)]
    valid: bool,

    /// Write the examples to a file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,

    /// Copy the examples JSON to the clipboard.
    #[arg(long)]
    clipboard: bool,

    /// Print the examples JSON to stdout, even when writing to a file or clipboard.
    #[arg(long)]
    print: bool,

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
    match cli.command {
        Some(Command::Generate(args)) => generate(args),
        Some(Command::Examples(args)) => examples(args),
        None => generate(GenerateArgs {
            files: cli.files,
            class_name: cli.class_name,
            out: cli.out,
            clipboard: cli.clipboard,
            print: cli.print,
            allow_additional: cli.allow_additional,
        }),
    }
}

fn generate(args: GenerateArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.files.is_empty() {
        return Err("at least one TypeScript file is required".into());
    }

    let source = read_sources(&args.files)?;
    let schema = schema_from_source(
        &source,
        SchemaOptions {
            root_type: args.class_name,
            additional_properties: args.allow_additional,
        },
    )?;
    let output = serde_json::to_string_pretty(&schema)?;

    write_output(&output, args.out, args.clipboard, args.print)?;

    Ok(())
}

fn examples(args: ExamplesArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.count == 0 {
        return Err("example count must be greater than zero".into());
    }

    let source = read_sources(&args.files)?;
    let schema = schema_from_source(
        &source,
        SchemaOptions {
            root_type: args.class_name,
            additional_properties: args.allow_additional,
        },
    )?;
    let examples = examples_from_schema(
        &schema,
        ExampleOptions {
            count: args.count,
            valid: args.valid || !args.invalid,
        },
    );
    let output = serde_json::to_string_pretty(&examples)?;

    write_output(&output, args.out, args.clipboard, args.print)?;

    Ok(())
}

fn read_sources(files: &[PathBuf]) -> Result<String, Box<dyn std::error::Error>> {
    let mut source = String::new();
    for path in files {
        source.push_str(&fs::read_to_string(path)?);
        source.push('\n');
    }
    Ok(source)
}

fn write_output(
    output: &str,
    out: Option<PathBuf>,
    clipboard: bool,
    print: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let wrote_file = out.is_some();
    if let Some(path) = out {
        fs::write(path, format!("{output}\n"))?;
    }

    if clipboard {
        let mut clipboard = arboard::Clipboard::new()?;
        clipboard.set_text(output.to_string())?;
    }

    if print || (!clipboard && !wrote_file) {
        println!("{output}");
    }

    Ok(())
}
