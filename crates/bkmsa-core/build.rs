fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);

    let proto_root = "proto";
    let mut config = prost_build::Config::new();
    config.type_attribute(
        ".",
        "#[derive(serde::Serialize, serde::Deserialize)] #[serde(rename_all = \"camelCase\")]",
    );
    config.compile_protos(
        &[
            "proto/spark/spark.proto",
            "proto/spark/spark_sampler.proto",
            "proto/spark/spark_heap.proto",
        ],
        &[proto_root],
    )?;
    println!("cargo:rerun-if-changed=proto/spark/spark.proto");
    println!("cargo:rerun-if-changed=proto/spark/spark_sampler.proto");
    println!("cargo:rerun-if-changed=proto/spark/spark_heap.proto");
    Ok(())
}
