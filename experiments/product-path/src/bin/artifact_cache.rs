use std::{env, error::Error, path::PathBuf, process};

use raccord_cache::{ArtifactStore, CacheKey};
use raccord_media::ArtifactRef;
use serde_json::json;

fn main() {
    if let Err(error) = run() {
        eprintln!("artifact-cache: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().collect();
    match arguments.get(1).map(String::as_str) {
        Some("lookup") => lookup(&arguments),
        Some("publish") => publish(&arguments),
        Some(_) | None => Err(usage().into()),
    }
}

fn lookup(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    if arguments.len() != 5 {
        return Err(usage().into());
    }
    let store = ArtifactStore::new(PathBuf::from(&arguments[2]))?;
    let key = cache_key(&arguments[3])?;
    let destination = PathBuf::from(&arguments[4]);
    let result = match store.copy_to(&key, &destination)? {
        Some(entry) => json!({
            "status": "hit",
            "byte_len": entry.metadata.byte_len,
        }),
        None => json!({ "status": "miss" }),
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn publish(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    if arguments.len() != 6 {
        return Err(usage().into());
    }
    let store = ArtifactStore::new(PathBuf::from(&arguments[2]))?;
    let key = cache_key(&arguments[3])?;
    let artifact = ArtifactRef::new(&arguments[4]).ok_or("artifact digest must not be empty")?;
    let source = PathBuf::from(&arguments[5]);
    let entry = store.put_file(&key, artifact, &source)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "status": "published",
            "byte_len": entry.metadata.byte_len,
        }))?
    );
    Ok(())
}

fn cache_key(value: &str) -> Result<CacheKey, Box<dyn Error>> {
    CacheKey::new(value.to_owned()).ok_or_else(|| format!("invalid cache key: {value}").into())
}

fn usage() -> &'static str {
    "usage: artifact_cache lookup <root> <key> <destination> | artifact_cache publish <root> <key> <digest> <source>"
}
