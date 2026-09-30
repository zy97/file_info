use clap::Parser;
use md5::{Digest as Md5Digest, Md5};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use walkdir::WalkDir;

/// 文件信息工具 - 分析指定路径的文件信息
#[derive(Parser, Debug)]
#[command(name = "file_info")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 要分析的路径
    #[arg(value_name = "PATH")]
    path: PathBuf,

    /// 要忽略的路径列表(可多次指定)
    #[arg(short, long, value_name = "IGNORE_PATH")]
    ignore: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Cache {
    version: u32,
    scanned_root: String,
    files: HashMap<String, CacheEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CacheEntry {
    size: u64,
    mtime: u64,
    hash: String,
}

fn should_ignore(path: &Path, ignore_paths: &[PathBuf]) -> bool {
    for ignore_path in ignore_paths {
        if path.starts_with(ignore_path) {
            return true;
        }
    }
    false
}

fn get_timestamp(system_time: SystemTime) -> u64 {
    system_time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn calculate_md5(file_path: &Path) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Md5::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn get_cache_path(root: &Path) -> Option<PathBuf> {
    let exe_path = env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;
    let cache_dir = exe_dir.join("file_info.cache");
    fs::create_dir_all(&cache_dir).ok()?;

    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_str = canonical_root.to_string_lossy();
    let hash = bytes_to_hex(&Sha256::digest(root_str.as_bytes()));

    Some(cache_dir.join(format!("scan_{}.json", hash)))
}

fn load_cache(path: &Path) -> Option<Cache> {
    let content = fs::read_to_string(path).ok()?;
    let cache: Cache = serde_json::from_str(&content).ok()?;
    if cache.version != 1 {
        return None;
    }
    Some(cache)
}

fn save_cache(path: &Path, cache: &Cache) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    let content = serde_json::to_string_pretty(cache)?;
    fs::write(&temp_path, content)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

fn main() {
    let args = Args::parse();

    if !args.ignore.is_empty() {
        println!("忽略路径:");
        for ignore_path in &args.ignore {
            println!("  - {}", ignore_path.display());
        }
    }

    let cache_path = get_cache_path(&args.path);
    let old_cache = Arc::new(cache_path.as_ref().and_then(|p| load_cache(p)).unwrap_or_default());
    let root = args.path.clone();

    let new_cache = Arc::new(Mutex::new(Cache {
        version: 1,
        scanned_root: root.to_string_lossy().to_string(),
        files: HashMap::new(),
    }));

    // 收集所有文件路径
    let files: Vec<_> = WalkDir::new(&root)
        .into_iter()
        .filter_entry(|e| !should_ignore(e.path(), &args.ignore))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    // 并行处理文件
    files.par_iter().for_each(|entry| {
        let path = entry.path();
        let rel_path = path.strip_prefix(&root).unwrap_or(path);
        let rel_key = rel_path.to_string_lossy().replace('\\', "/");

        match entry.metadata() {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => {
                    let timestamp = get_timestamp(modified);
                    let size = metadata.len();

                    let hash = match old_cache.files.get(&rel_key) {
                        Some(cached) if cached.size == size && cached.mtime == timestamp => {
                            cached.hash.clone()
                        }
                        _ => match calculate_md5(path) {
                            Ok(h) => h,
                            Err(e) => {
                                eprintln!("无法计算MD5 {}: {}", path.display(), e);
                                return;
                            }
                        },
                    };

                    println!("{}\t{}\t{}", timestamp, hash, path.display());

                    if let Ok(mut cache) = new_cache.lock() {
                        cache.files.insert(
                            rel_key,
                            CacheEntry {
                                size,
                                mtime: timestamp,
                                hash,
                            },
                        );
                    }
                }
                Err(e) => {
                    eprintln!("无法获取修改时间 {}: {}", path.display(), e);
                }
            },
            Err(e) => {
                eprintln!("无法读取元数据 {}: {}", path.display(), e);
            }
        }
    });

    if let Some(cp) = cache_path {
        if let Ok(cache) = Arc::try_unwrap(new_cache) {
            if let Ok(cache) = cache.into_inner() {
                if let Err(e) = save_cache(&cp, &cache) {
                    eprintln!("无法保存缓存文件 {}: {}", cp.display(), e);
                }
            }
        }
    }
}
