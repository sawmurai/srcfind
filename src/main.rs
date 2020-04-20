#![allow(clippy::must_use_candidate)]

use crate::parser::Parser;
use crate::scanner::Scanner;
use spmc::{channel, Sender};
use std::collections::HashMap;
use std::fs::{self};
use std::io::{self, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

pub mod environment;
pub mod expression;
pub mod parser;
pub mod scanner;
pub mod token;

type ParseMessage = (String, String);
type ParseMessageSender = Sender<ParseMessage>;

fn visit_file(path: &Path, tx: &mut ParseMessageSender) -> io::Result<()> {
    if let Some(ext) = path.extension() {
        if ext == "php" {
            let p = path.to_str().unwrap().to_string();

            if let Ok(content) = fs::read_to_string(path) {
                tx.send((p, content)).unwrap();
            }
            //println!("{:#?}", parser.ast());
            //if let Err(msg) = index_file(&p, file_registry.add(&p), t) {
            //    eprintln!("Could not read file {}: {}", &p, &msg);
            //}
        }
    }

    Ok(())
}

fn visit_dirs(dir: &Path, tx: &mut ParseMessageSender) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // if path.ends_with("vendor") {
            //    continue;
            // }

            if path.is_dir() {
                visit_dirs(&path, tx)?;
            } else {
                visit_file(&path, tx)?;
            }
        }
    } else {
        visit_file(&dir, tx)?;
    }

    Ok(())
}

fn main() -> Result<()> {
    let d: HashMap<String, environment::Environment> = HashMap::new();
    let registry = Arc::new(Mutex::new(d));

    let (mut tx, rx) = channel::<ParseMessage>();
    let num_threads = 6;

    let mut handles = Vec::new();
    for _ in 0..num_threads {
        let rx = rx.clone();

        let registry = Arc::clone(&registry);
        let handle = thread::spawn(move || loop {
            let (p, content) = rx.recv().unwrap();

            if p == "" {
                return;
            }

            let mut scanner = Scanner::new(&content);

            if let Err(msg) = scanner.scan() {
                eprintln!("Could not read file {}: {}", &p, &msg);
            }

            // Later on we need to generate an AST, as well as an environment and the
            // symbol table. This will then replace the token streams
            //t.insert(p, scanner.tokens);

            //                println!("{:#?}", &scanner.tokens);
            // Prototype to tell the token from the current position of the cursor
            let result = Parser::ast(scanner.tokens);

            //println!("{:#?}", &scanner.tokens);
            if let Err(e) = result {
                println!("{}: {:#?}", p, e);
            } else if let Ok((ast, errors)) = result {
                let mut env = environment::Environment::default();

                env.enter_scope(&p);
                environment::index::walk_tree(&mut env, ast);

                env.finish_scope();
                //println!("{:#?}", &env);
                //registry.lock().unwrap().insert(p, env);

                if !errors.is_empty() {
                    //println!("Parsing {}", p);

                    println!("Parsing {}\n{:#?}", p, errors);
                }
            } else {
                //println!("{:#?}", result);
            }
        });

        handles.push(handle);
    }

    visit_dirs(
        Path::new(&std::env::args().nth(1).unwrap_or_else(|| String::from("."))),
        &mut tx,
    )?;

    for _ in 0..num_threads {
        tx.send(("".to_string(), "".to_string())).unwrap();
    }

    for handle in handles {
        handle.join().unwrap();
    }
    Ok(())
}
