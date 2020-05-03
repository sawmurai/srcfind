use crate::environment::import::SymbolImport;
use crate::environment::symbol::{document_symbol, Symbol};
use crate::node::{get_range, Node};
use crate::token::Token;
use async_recursion::async_recursion;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::{Diagnostic, Location, Range, Url};

pub enum ScopeType {
    Function,
    Class,
    Method,
    Global,
    Namespace,
    File,
}

impl Default for ScopeType {
    fn default() -> ScopeType {
        ScopeType::Global
    }
}

#[derive(Clone, Debug)]
pub struct Reference {
    /// Used for variables
    pub token: Option<Token>,

    /// The type_ref if applicable
    pub type_ref: Option<Vec<Token>>,

    /// The definition of the symbol that is used
    pub defining_symbol: Option<Symbol>,

    /// Selection range of the usage
    pub range: Range,
}

impl Reference {
    /// Reference to a variable
    pub fn variable(token: &Token) -> Self {
        Self {
            token: Some(token.clone()),
            defining_symbol: None,
            type_ref: None,
            range: get_range(token.range()),
        }
    }

    /// Reference to an identifier, for example a function or a member
    pub fn identifier(token: &Token) -> Self {
        Self {
            token: Some(token.clone()),
            defining_symbol: None,
            type_ref: None,
            range: get_range(token.range()),
        }
    }

    /// Reference to a type
    pub fn type_ref(type_ref: Vec<Token>) -> Self {
        let range = get_range((
            type_ref.first().unwrap().range().0,
            type_ref.last().unwrap().range().1,
        ));

        Self {
            token: None,
            defining_symbol: None,
            type_ref: Some(type_ref),
            range,
        }
    }
}

impl From<Reference> for Diagnostic {
    fn from(reference: Reference) -> Diagnostic {
        Diagnostic {
            range: reference.range,
            message: String::from("Missing"),
            ..Diagnostic::default()
        }
    }
}

#[derive(Default)]
pub struct Scope {
    /// Type of this scope
    pub scope_type: ScopeType,

    /// Symbols defined in this scope
    pub symbols: HashMap<String, Symbol>,

    /// Symbols imported into this scope via use-statements
    pub imports: Vec<SymbolImport>,

    /// Access to this scope's parent
    pub parent: Option<Arc<Mutex<Scope>>>,

    /// All children. The key is the function-, class- or namespace-name
    pub children: HashMap<String, Arc<Mutex<Scope>>>,

    /// All unresolved references
    pub references: Vec<Reference>,
}

impl Scope {
    pub async fn within(
        name: &str,
        parent: Arc<Mutex<Self>>,
        scope_type: ScopeType,
    ) -> Arc<Mutex<Self>> {
        let new = Self {
            parent: Some(parent.clone()),
            scope_type,
            ..Default::default()
        };

        let new = Arc::new(Mutex::new(new));

        parent
            .lock()
            .await
            .children
            .insert(name.to_owned(), Arc::clone(&new));

        new
    }

    pub fn definition(&mut self, symbol: Symbol) {
        self.symbols.insert(symbol.name.clone(), symbol);
    }

    #[async_recursion]
    pub async fn reference(&mut self, token: &Token, location: Location) -> Result<(), String> {
        let name = if let Some(name) = token.label.as_ref() {
            name
        } else {
            return Err("Usage of token without label is impossible".to_owned());
        };

        if let Some(definition) = self.symbols.get_mut(name) {
            definition.reference(location);

            return Ok(());
        } else if let Some(parent) = self.parent.as_ref() {
            return parent.lock().await.reference(token, location).await;
        }

        return Err("Usage of undefined symbol!".to_owned());
    }

    pub fn get_definitions(&self) -> Vec<Symbol> {
        self.symbols
            .values()
            .map(|r| r.clone())
            .collect::<Vec<Symbol>>()
    }

    pub fn get_unresolvable(&self) -> Vec<Reference> {
        self.references
            .iter()
            .filter(|r| r.defining_symbol.is_none())
            .map(|s| s.clone())
            .collect::<Vec<Reference>>()
    }

    #[async_recursion]
    pub async fn all_definitions(&self) -> Vec<Symbol> {
        let mut symbols: Vec<Symbol> = self.get_definitions();

        for scope in self.children.values() {
            let child = scope.lock().await;

            symbols.extend(child.all_definitions().await);
        }

        symbols
    }

    #[async_recursion]
    pub async fn all_unresolvable(&self) -> Vec<Reference> {
        let mut references: Vec<Reference> = self.get_unresolvable();

        for scope in self.children.values() {
            let child = scope.lock().await;

            references.extend(child.all_unresolvable().await);
        }

        references
    }

    pub fn prepare_reference(&mut self, reference: Reference) {
        self.references.push(reference);
    }

    #[async_recursion]
    pub async fn resolve(&self, reference: &Reference) -> Option<Symbol> {
        let ref_name = if let Some(token) = reference.token.as_ref() {
            if let Some(label) = token.label.as_ref() {
                label
            } else {
                ""
            }
        } else {
            ""
        };

        for (name, symbol) in self.symbols.iter() {
            if name == &ref_name {
                return Some(symbol.clone());
            }
        }

        if let Some(parent) = self.parent.as_ref() {
            let parent = parent.lock().await;

            return parent.resolve(reference).await;
        }

        None
    }
}

#[async_recursion]
pub async fn collect_symbols(node: &Node, scope: Arc<Mutex<Scope>>) -> Result<(), String> {
    match node {
        Node::NamespaceStatement { .. }
        | Node::Function { .. }
        | Node::FunctionArgument { .. }
        | Node::Class { .. }
        | Node::NamespaceBlock { .. }
        | Node::ClassStatement { .. }
        | Node::TraitStatement { .. }
        | Node::ClassConstantDefinitionStatement { .. }
        | Node::PropertyDefinitionStatement { .. }
        | Node::MethodDefinitionStatement { .. }
        | Node::FunctionDefinitionStatement { .. }
        | Node::NamedFunctionDefinitionStatement { .. }
        | Node::Const { .. }
        | Node::Interface { .. } => {
            let def = document_symbol(node, scope.clone()).await?;

            scope.lock().await.definition(def);
        }

        Node::LexicalVariable { variable, .. }
        | Node::Variable(variable)
        | Node::StaticVariable { variable, .. } => {
            let def = document_symbol(node, scope.clone()).await?;

            let mut scope = scope.lock().await;
            scope.definition(def);
            scope.prepare_reference(Reference::variable(variable));
        }
        Node::Identifier(token) => scope.lock().await.definition(Symbol::from(token)),
        Node::Literal(token) => {
            scope
                .lock()
                .await
                .prepare_reference(Reference::identifier(token));
        }
        Node::TypeRef(parts) => {
            scope.lock().await.prepare_reference(Reference::type_ref(
                parts
                    .iter()
                    .filter(|t| t.label.is_some())
                    .map(|t| t.clone())
                    .collect(),
            ));
        }
        _ => {
            for child in node.children() {
                collect_symbols(child, scope.clone()).await?;
            }
        }
    };

    Ok(())
}
#[async_recursion]
pub async fn resolve_references(uri: &Url, scope: Arc<Mutex<Scope>>) -> Result<(), String> {
    let scope = scope.lock().await;

    for reference in scope.references.iter().as_ref() {
        scope.resolve(&reference).await;
    }

    for child in scope.children.values() {
        resolve_references(uri, child.clone()).await?;
    }

    Ok(())
}
