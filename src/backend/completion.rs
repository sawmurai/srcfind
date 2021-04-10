use super::BackendState;
use crate::environment::fs as EnvFs;
use crate::environment::get_range;
use crate::{environment::symbol::PhpSymbolKind, suggester};

use lsp_types::CompletionContext;
use suggester::SuggestionContext;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{CompletionItem, CompletionParams, CompletionResponse, TextEdit};

fn get_trigger(context: Option<CompletionContext>) -> Option<char> {
    if let Some(context) = context {
        if let Some(tc) = context.trigger_character {
            tc.chars().nth(0)
        } else {
            None
        }
    } else {
        None
    }
}

pub(crate) fn completion(
    state: &BackendState,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let pos = params.text_document_position.position;

    let opened_file = &EnvFs::normalize_path(
        &params
            .text_document_position
            .text_document
            .uri
            .to_file_path()
            .unwrap(),
    );

    let trigger = get_trigger(params.context);
    let file_ast = state.opened_files.get(opened_file);

    if let Some((file_ast, _range)) = file_ast {
        let ast = file_ast;

        let current_file_symbol = if let Some(current_file_symbol) = state.files.get(opened_file) {
            current_file_symbol
        } else {
            return Ok(None);
        };
        let current_file = state.arena[*current_file_symbol].get();

        let symbol_under_cursor = current_file.symbol_at(&pos, *current_file_symbol, &state.arena);

        if let Some(references) = state.symbol_references.get(opened_file) {
            let mut suggestions = suggester::get_suggestions_at(
                trigger,
                pos,
                symbol_under_cursor,
                ast,
                &state.arena,
                &state.global_symbols,
                references,
            );

            return Ok(Some(CompletionResponse::Array(
                suggestions
                    .drain(..)
                    .map(|sug| {
                        if let Some(token) = sug.token {
                            return token.into();
                        }

                        let sn = sug.node.unwrap();
                        let symbol = state.arena[sn].get();

                        if symbol.kind == PhpSymbolKind::Class
                            || symbol.kind == PhpSymbolKind::Trait
                        {
                            if sug.context == SuggestionContext::Import {
                                return CompletionItem {
                                    additional_text_edits: Some(vec![TextEdit {
                                        range: get_range(sug.replace.unwrap()),
                                        new_text: String::from(""),
                                    }]),
                                    label: symbol.fqdn(),
                                    ..symbol.completion_item(sn, &state.arena)
                                };
                            }

                            // If the symbol is a class we try to add a namespace as a text edit
                            let ns = if let Some(ns) = symbol.namespace.as_ref() {
                                ns
                            } else {
                                return symbol.completion_item(sn, &state.arena);
                            };

                            // Same namespace, no need to add an import
                            if current_file.namespace.eq(&symbol.namespace) {
                                return symbol.completion_item(sn, &state.arena);
                            }

                            let fqdn = symbol.fqdn();
                            // Check if the current file already has that import. if yes we are good
                            let line = if let Some(imports) = current_file.imports.as_ref() {
                                if imports
                                    .all()
                                    .find(|import| import.full_name() == fqdn)
                                    .is_some()
                                {
                                    return symbol.completion_item(sn, &state.arena);
                                } else {
                                    // add use to the end of the imports
                                    if let Some(first_import) = imports.all().nth(0) {
                                        first_import.path.range().0 .0
                                    } else {
                                        3
                                    }
                                }
                            } else {
                                // add use right after the namespace or the opening <?php
                                3
                            };

                            // if not, we add it as a text edit
                            return CompletionItem {
                                additional_text_edits: Some(vec![TextEdit {
                                    range: get_range(((line, 0), (line, 0))),
                                    new_text: format!("use {};\n", fqdn),
                                }]),
                                ..symbol.completion_item(sn, &state.arena)
                            };
                        }

                        let mut item = symbol.completion_item(sn, &state.arena);
                        if let Some(alias) = sug.alias {
                            item.label = alias;
                        }
                        item
                    })
                    .collect::<Vec<CompletionItem>>(),
            )));
        }
    }

    Ok(None)
}