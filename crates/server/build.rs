//! 生成 agent gRPC 的**服务端**代码(agent 侧由 codeplan-agent 生成 client)。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_client(false)
        .compile_protos(&["../../proto/codeplan/agent/v1/agent.proto"], &["../../proto"])?;
    Ok(())
}
