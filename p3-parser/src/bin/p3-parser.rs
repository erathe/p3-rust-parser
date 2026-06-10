use clap::Parser as ClapParser;
use p3_parser::{Message, StreamParser};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[derive(ClapParser)]
#[command(name = "p3-parser")]
#[command(about = "Parse MyLaps ProChip P3 binary protocol messages to JSON")]
struct Args {
    #[arg(short = 'H', long, default_value = "localhost")]
    host: String,

    #[arg(short, long, default_value = "5403")]
    port: u16,

    #[arg(long)]
    pretty: bool,

    /// Print all message types (STATUS, VERSION), not just PASSING.
    /// Lines are tagged with a "message_type" field.
    #[arg(long)]
    all: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    eprintln!("Connecting to {}:{}...", args.host, args.port);

    let mut stream = TcpStream::connect((args.host.as_str(), args.port)).await?;
    eprintln!("Connected!");

    let mut parser = StreamParser::new();

    loop {
        // Read data from stream
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await?;

        if n == 0 {
            eprintln!("Connection closed");
            break;
        }

        parser.feed(&chunk[..n]);

        while let Some(result) = parser.next_message() {
            match result {
                Ok(message) => {
                    if args.all {
                        // Tagged with "message_type" so types are distinguishable
                        print_json(&message, args.pretty)?;
                    } else if let Message::Passing(passing_message) = &message {
                        print_json(passing_message, args.pretty)?;
                    }
                }
                Err(e) => {
                    eprintln!("Parse error: {}", e);
                }
            }
        }
    }

    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T, pretty: bool) -> anyhow::Result<()> {
    let json = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    println!("{}", json);
    Ok(())
}
