use crate::scanner::Scanner;

use crate::token::Token;
use std::collections::HashMap;
use std::fs::{self};
use std::io::{self, Result};
use std::path::Path;

pub mod scanner;
pub mod token;

fn visit_dirs(dir: &Path, t: &mut HashMap<String, Vec<Token>>) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                visit_dirs(&path, t)?;
            } else {
                if let Some(ext) = path.extension() {
                    if ext == "php" {
                        let p = path.to_str().unwrap().to_string();

                        let content = match fs::read_to_string(path) {
                            Ok(content) => content,
                            Err(error) => {
                                eprintln!("{}", error);

                                continue;
                            }
                        };
                        let mut scanner = Scanner::new(&content);

                        if let Err(msg) = scanner.scan() {
                            eprintln!("Could not read file {}: {}", &p, &msg);
                        }

                        // Later on we need to generate an AST, as well as an environment and the
                        // symbol table. This will then replace the token streams
                        t.insert(p, scanner.tokens);
                        //if let Err(msg) = index_file(&p, file_registry.add(&p), t) {
                        //    eprintln!("Could not read file {}: {}", &p, &msg);
                        //}
                    }
                }
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {    
    let mut token_stream_map: HashMap<String, Vec<Token>> = HashMap::new();

    visit_dirs(
        Path::new(&std::env::args().nth(1).unwrap_or(String::from("."))),
        &mut token_stream_map,
    )?;
    println!("Indexed!");
    
    Ok(())
}
