// Example to check type sizes for performance analysis
use std::mem::size_of;

fn main() {
    // Print sizes of all types
    println!("=== Type Sizes ===");
    println!("Error: {} bytes", size_of::<mcp_core::Error>());
    println!("Result<()>: {} bytes", size_of::<mcp_core::Result<()>>());
    println!("Result<u64>: {} bytes", size_of::<mcp_core::Result<u64>>());

    println!("\n=== Domain Types ===");
    println!("ServerId: {} bytes", size_of::<mcp_core::ServerId>());
    println!("ToolName: {} bytes", size_of::<mcp_core::ToolName>());

    println!("\n=== Config Types ===");
    println!(
        "ServerConfig: {} bytes",
        size_of::<mcp_core::ServerConfig>()
    );
    println!(
        "TransportType: {} bytes",
        size_of::<mcp_core::TransportType>()
    );

    println!("\n=== Standard Types (Reference) ===");
    println!("String: {} bytes", size_of::<String>());
    println!("Option<String>: {} bytes", size_of::<Option<String>>());
    println!(
        "Box<dyn std::error::Error + Send + Sync>: {} bytes",
        size_of::<Box<dyn std::error::Error + Send + Sync>>()
    );
}
