//! 生成 agent 的 gRPC 客户端代码(平台侧由 server 生成 server 端)。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(false) // agent 侧只做 client
        .compile_protos(&["../../proto/codeplan/agent/v1/agent.proto"], &["../../proto"])?;
    Ok(())
}
