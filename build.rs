extern crate glob;
extern crate prost_build;

use self::glob::glob;
use anyhow::Result;
use flate2::bufread::GzEncoder;
use flate2::Compression;
use regex::Regex;
use std::env;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use vergen_gitcl::{BuildBuilder, CargoBuilder, Emitter, GitclBuilder, RustcBuilder};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=./protobufs");
    println!("cargo:rerun-if-changed=./migrations");

    let files: Vec<_> = glob("protobufs/meshtastic/*.proto")
        .unwrap()
        .map(|p| p.unwrap().to_str().unwrap().to_string())
        .collect();

    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[allow(dead_code)]");
    config.compile_protos(&files[..], &["protobufs/"])?;
    build_svg_symbol_sheet()?;
    compress_static_files()?;
    vergen()?;
    Ok(())
}

fn vergen() -> Result<()> {
    let build = BuildBuilder::all_build()?;
    let cargo = CargoBuilder::all_cargo()?;
    let git = GitclBuilder::all_git()?;
    let rustc = RustcBuilder::all_rustc()?;

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&git)?
        .add_instructions(&rustc)?
        .emit()?;

    Ok(())
}

fn compress_static_files() -> Result<()> {
    println!("cargo:rerun-if-changed=./static");
    let static_pattern = "static/*";
    let out_path = Path::new(&env::var("OUT_DIR").unwrap()).to_owned();

    for entry in glob(static_pattern).expect("Failed to read glob pattern") {
        if let Ok(path) = entry
            && let Some(path_str) = path.to_str() {
                let target = out_path.join(path_str);
                let directory = target
                    .components()
                    .collect::<Vec<_>>()
                    .split_last()
                    .unwrap()
                    .1
                    .iter()
                    .fold(PathBuf::new(), |buf, comp| buf.join(comp));
                if !directory.exists() {
                    fs::create_dir_all(directory)?;
                }

                if path_str.ends_with(".js") || path_str.ends_with(".css") {
                    let input = BufReader::new(File::open(path_str)?);
                    let f = File::create(target.to_str().unwrap().to_owned() + ".gz")?;
                    let mut output = BufWriter::new(f);
                    let mut gz = GzEncoder::new(input, Compression::default());
                    let mut buffer = Vec::new();
                    gz.read_to_end(&mut buffer)?;
                    output.write_all(&buffer)?;
                    output.flush()?;
                } else {
                    fs::copy(path_str, target).unwrap();
                }
            }
    }

    // Output the path for use in include_str!
    println!(
        "cargo:rustc-env=PROCESSED_STATIC_PATH={}",
        out_path.join("static").to_str().unwrap()
    );

    Ok(())
}

fn build_svg_symbol_sheet() -> Result<()> {
    // Pattern for SVG files in the directory
    println!("cargo:rerun-if-changed=./icons");
    let svg_pattern = "icons/*.svg";
    // Output SVG file path
    let out_path = Path::new(&env::var("OUT_DIR").unwrap()).join("icons.svg");
    let mut outfile = File::create(out_path)?;

    writeln!(
        outfile,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" style=\"display: none;\">"
    )?;

    let re = Regex::new(r"/?([^/]+)\.svg$").unwrap();
    for entry in glob(svg_pattern).expect("Failed to read glob pattern") {
        if let Ok(path) = entry {
            // Convert the path to a string for regex matching
            if let Some(path_str) = path.to_str()
                && let Some(caps) = re.captures(path_str) {
                    // Extract the file name without the extension
                    let id = caps.get(1).unwrap().as_str();
                    let content = fs::read_to_string(&path)?;
                    // Create the symbol content
                    let symbol_content = content
                        .replace("<svg", &format!("<symbol id=\"icon-{}\"", id))
                        .replace("</svg>", "</symbol>")
                        .replace(" xmlns=\"http://www.w3.org/2000/svg\"", "");

                    writeln!(outfile, "{}", symbol_content)?;
                }
        }
    }

    writeln!(outfile, "</svg>")?;

    let out_path = Path::new(&env::var("OUT_DIR").unwrap()).join("icons.svg");

    // Output the path for use in include_str!
    println!(
        "cargo:rustc-env=SVG_ICONS_PATH={}",
        out_path.to_str().unwrap()
    );

    Ok(())
}
