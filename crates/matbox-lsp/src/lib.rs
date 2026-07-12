//! MATBOX Language Server Protocol implementation.
//! Communicates via JSON-RPC over stdin/stdout.

use matbox_core::{Engine, Parser};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};

// ── JSON-RPC types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcMessage {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
}

// ── LSP types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct InitializeResult {
    capabilities: ServerCapabilities,
    server_info: ServerInfo,
}

#[derive(Serialize)]
struct ServerCapabilities {
    text_document_sync: u64,
    completion_provider: CompletionOptions,
    hover_provider: bool,
    definition_provider: bool,
}

#[derive(Serialize)]
struct CompletionOptions {
    trigger_characters: Vec<String>,
}

#[derive(Serialize)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct Diagnostic {
    range: Range,
    severity: Option<u64>,
    source: Option<String>,
    message: String,
}

#[derive(Serialize)]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Serialize, Deserialize, Clone)]
struct Position {
    line: u64,
    character: u64,
}

#[derive(Serialize)]
struct PublishDiagnosticsParams {
    uri: String,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
struct CompletionList {
    is_incomplete: bool,
    items: Vec<CompletionItem>,
}

#[derive(Serialize)]
struct CompletionItem {
    label: String,
    kind: Option<u64>,
    detail: Option<String>,
    insert_text: Option<String>,
}

#[derive(Serialize)]
struct Hover {
    contents: HoverContents,
}

#[derive(Serialize)]
struct HoverContents {
    kind: String,
    value: String,
}

// ── LSP Server ──────────────────────────────────────────────────────────

pub fn run() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut engine = Engine::new();
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        if stdin.lock().read_line(&mut line_buf).unwrap_or(0) == 0 {
            break;
        }
        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        // Parse Content-Length header
        if line.starts_with("Content-Length:") {
            let len: usize = line
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            // Read blank line
            let mut blank = String::new();
            stdin.lock().read_line(&mut blank).unwrap();
            // Read content
            let mut content = vec![0u8; len];
            if stdin.lock().read_exact(&mut content).is_err() {
                break;
            }
            let msg_str = String::from_utf8_lossy(&content);
            if let Ok(msg) = serde_json::from_str::<JsonRpcMessage>(&msg_str) {
                handle_message(&msg, &mut documents, &mut engine, &mut stdout);
            }
        }
    }
}

fn handle_message(
    msg: &JsonRpcMessage,
    documents: &mut HashMap<String, String>,
    _engine: &mut Engine,
    stdout: &mut io::Stdout,
) {
    match msg.method.as_str() {
        "initialize" => {
            let result = InitializeResult {
                capabilities: ServerCapabilities {
                    text_document_sync: 1, // Full document sync
                    completion_provider: CompletionOptions {
                        trigger_characters: vec![
                            ".".into(),
                            "@".into(),
                            "(".into(),
                        ],
                    },
                    hover_provider: true,
                    definition_provider: true,
                },
                server_info: ServerInfo {
                    name: "matbox-lsp".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            };
            send_response(msg.id.unwrap(), Some(serde_json::to_value(&result).unwrap()), None, stdout);
        }
        "textDocument/didOpen" | "textDocument/didChange" => {
            let uri = msg
                .params
                .pointer("/textDocument/uri")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = msg
                .params
                .pointer("/textDocument/text")
                .or_else(|| msg.params.pointer("/contentChanges/0/text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            documents.insert(uri.clone(), text.clone());
            publish_diagnostics(&uri, &text, stdout);
        }
        "textDocument/completion" => {
            let result = CompletionList {
                is_incomplete: false,
                items: builtin_completions(),
            };
            send_response(msg.id.unwrap(), Some(serde_json::to_value(&result).unwrap()), None, stdout);
        }
        "textDocument/hover" => {
            let word = extract_word_at_cursor(msg, documents);
            let hover = Hover {
                contents: HoverContents {
                    kind: "markdown".into(),
                    value: if !word.is_empty() {
                        builtin_doc(&word)
                    } else {
                        "MATBOX expression".into()
                    },
                },
            };
            send_response(msg.id.unwrap(), Some(serde_json::to_value(&hover).unwrap()), None, stdout);
        }
        "textDocument/definition" => {
            // For now, return null (no definition support beyond builtins)
            send_response(msg.id.unwrap(), None, None, stdout);
        }
        "shutdown" => {
            send_response(msg.id.unwrap(), Some(serde_json::Value::Null), None, stdout);
        }
        "exit" => {
            std::process::exit(0);
        }
        _ => {
            // Method not supported — respond with null
            if let Some(id) = msg.id {
                send_response(id, None, None, stdout);
            }
        }
    }
}

fn publish_diagnostics(uri: &str, text: &str, stdout: &mut io::Stdout) {
    let mut diagnostics = Vec::new();
    match Parser::new(text) {
        Ok(mut p) => {
            if let Err(e) = p.parse_program() {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: 0, character: 0 },
                        end: Position { line: 0, character: 0 },
                    },
                    severity: Some(1),
                    source: Some("matbox".into()),
                    message: e,
                });
            }
        }
        Err(e) => {
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 0, character: 0 },
                },
                severity: Some(1),
                source: Some("matbox".into()),
                message: e,
            });
        }
    }
    let params = PublishDiagnosticsParams {
        uri: uri.to_string(),
        diagnostics,
    };
    send_notification("textDocument/publishDiagnostics", &params, stdout);
}

fn extract_word_at_cursor(msg: &JsonRpcMessage, documents: &HashMap<String, String>) -> String {
    let uri = msg
        .params
        .pointer("/textDocument/uri")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let line: usize = msg
        .params
        .pointer("/position/line")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let col: usize = msg
        .params
        .pointer("/position/character")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    if let Some(text) = documents.get(uri) {
        if let Some(line_str) = text.lines().nth(line) {
            let chars: Vec<char> = line_str.chars().collect();
            if col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_') {
                let mut start = col;
                let mut end = col;
                while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                    start -= 1;
                }
                while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                return chars[start..end].iter().collect();
            }
        }
    }
    String::new()
}

fn builtin_completions() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let builtins = vec![
        ("sin", "Trigonometric sine", "Function"),
        ("cos", "Trigonometric cosine", "Function"),
        ("tan", "Trigonometric tangent", "Function"),
        ("asin", "Inverse sine", "Function"),
        ("acos", "Inverse cosine", "Function"),
        ("atan", "Inverse tangent", "Function"),
        ("atan2", "Four-quadrant inverse tangent", "Function"),
        ("sinh", "Hyperbolic sine", "Function"),
        ("cosh", "Hyperbolic cosine", "Function"),
        ("tanh", "Hyperbolic tangent", "Function"),
        ("exp", "Exponential", "Function"),
        ("log", "Natural logarithm", "Function"),
        ("log10", "Base-10 logarithm", "Function"),
        ("log2", "Base-2 logarithm", "Function"),
        ("sqrt", "Square root", "Function"),
        ("abs", "Absolute value", "Function"),
        ("angle", "Phase angle", "Function"),
        ("conj", "Complex conjugate", "Function"),
        ("real", "Real part", "Function"),
        ("imag", "Imaginary part", "Function"),
        ("floor", "Round toward negative infinity", "Function"),
        ("ceil", "Round toward positive infinity", "Function"),
        ("round", "Round to nearest integer", "Function"),
        ("fix", "Round toward zero", "Function"),
        ("sign", "Sign function", "Function"),
        ("mod", "Modulus", "Function"),
        ("rem", "Remainder", "Function"),
        ("erf", "Error function", "Function"),
        ("erfc", "Complementary error function", "Function"),
        ("erfinv", "Inverse error function", "Function"),
        ("gamma", "Gamma function", "Function"),
        ("gammaln", "Log gamma function", "Function"),
        ("length", "Length of vector", "Function"),
        ("size", "Array dimensions", "Function"),
        ("numel", "Number of elements", "Function"),
        ("ndims", "Number of dimensions", "Function"),
        ("sum", "Sum of elements", "Function"),
        ("prod", "Product of elements", "Function"),
        ("mean", "Mean value", "Function"),
        ("median", "Median value", "Function"),
        ("mode", "Most frequent value", "Function"),
        ("std", "Standard deviation", "Function"),
        ("var", "Variance", "Function"),
        ("min", "Minimum value", "Function"),
        ("max", "Maximum value", "Function"),
        ("any", "Any true", "Function"),
        ("all", "All true", "Function"),
        ("cumsum", "Cumulative sum", "Function"),
        ("cumprod", "Cumulative product", "Function"),
        ("diff", "Differences", "Function"),
        ("find", "Find nonzero indices", "Function"),
        ("sort", "Sort array", "Function"),
        ("unique", "Unique values", "Function"),
        ("flip", "Flip array", "Function"),
        ("fliplr", "Flip left-right", "Function"),
        ("flipud", "Flip up-down", "Function"),
        ("reshape", "Reshape array", "Function"),
        ("repmat", "Replicate array", "Function"),
        ("kron", "Kronecker product", "Function"),
        ("diag", "Diagonal", "Function"),
        ("cat", "Concatenate arrays", "Function"),
        ("linspace", "Linearly spaced vector", "Function"),
        ("logspace", "Logarithmically spaced vector", "Function"),
        ("meshgrid", "2D grid coordinates", "Function"),
        ("zeros", "Create zeros array", "Function"),
        ("ones", "Create ones array", "Function"),
        ("eye", "Identity matrix", "Function"),
        ("rand", "Uniform random", "Function"),
        ("randn", "Normal random", "Function"),
        ("randi", "Integer random", "Function"),
        ("randperm", "Random permutation", "Function"),
        ("magic", "Magic square", "Function"),
        ("inv", "Matrix inverse", "Function"),
        ("det", "Determinant", "Function"),
        ("rank", "Matrix rank", "Function"),
        ("trace", "Matrix trace", "Function"),
        ("norm", "Matrix norm", "Function"),
        ("lu", "LU decomposition", "Function"),
        ("qr", "QR decomposition", "Function"),
        ("chol", "Cholesky decomposition", "Function"),
        ("eig", "Eigenvalues and eigenvectors", "Function"),
        ("svd", "Singular value decomposition", "Function"),
        ("pinv", "Pseudoinverse", "Function"),
        ("polyval", "Evaluate polynomial", "Function"),
        ("poly", "Polynomial from roots", "Function"),
        ("roots", "Polynomial roots", "Function"),
        ("polyfit", "Polynomial curve fitting", "Function"),
        ("intersect", "Set intersection", "Function"),
        ("setdiff", "Set difference", "Function"),
        ("setxor", "Set XOR", "Function"),
        ("union", "Set union", "Function"),
        ("ismember", "Set membership", "Function"),
        ("issorted", "Check if sorted", "Function"),
        ("corrcoef", "Correlation coefficient", "Function"),
        ("cov", "Covariance", "Function"),
        ("fft", "Fast Fourier transform", "Function"),
        ("ifft", "Inverse FFT", "Function"),
        ("fftshift", "Shift zero frequency", "Function"),
        ("ifftshift", "Inverse shift", "Function"),
        ("conv", "Convolution", "Function"),
        ("filter", "Digital filter", "Function"),
        ("fzero", "Root finding", "Function"),
        ("trapz", "Trapezoidal integration", "Function"),
        ("integral", "Numerical integration", "Function"),
        ("strcmp", "String comparison", "Function"),
        ("strcmpi", "Case-insensitive comparison", "Function"),
        ("upper", "Uppercase", "Function"),
        ("lower", "Lowercase", "Function"),
        ("strtrim", "Trim whitespace", "Function"),
        ("deblank", "Remove trailing blanks", "Function"),
        ("strfind", "Find string in string", "Function"),
        ("strrep", "String replace", "Function"),
        ("regexp", "Regular expression match", "Function"),
        ("regexpi", "Case-insensitive regexp", "Function"),
        ("regexprep", "Regex replace", "Function"),
        ("strsplit", "Split string", "Function"),
        ("strjoin", "Join strings", "Function"),
        ("sprintf", "Format string", "Function"),
        ("num2str", "Number to string", "Function"),
        ("int2str", "Integer to string", "Function"),
        ("str2double", "String to double", "Function"),
        ("contains", "Check pattern", "Function"),
        ("startsWith", "Check start", "Function"),
        ("endsWith", "Check end", "Function"),
        ("disp", "Display value", "Function"),
        ("error", "Throw error", "Function"),
        ("warning", "Display warning", "Function"),
        ("who", "List variables", "Function"),
        ("clear", "Clear workspace", "Function"),
        ("exist", "Check existence", "Function"),
        ("format", "Set output format", "Function"),
        ("run", "Run script", "Function"),
        ("help", "Help", "Function"),
        ("input", "User input", "Function"),
        ("clc", "Clear console", "Function"),
        ("pause", "Pause execution", "Function"),
        ("tic", "Start timer", "Function"),
        ("toc", "End timer", "Function"),
        ("eval", "Evaluate string", "Function"),
        ("evalc", "Evaluate with capture", "Function"),
        ("feval", "Call function by name", "Function"),
        ("struct", "Create struct", "Function"),
        ("isfield", "Check field", "Function"),
        ("fieldnames", "List fields", "Function"),
        ("rmfield", "Remove field", "Function"),
        ("orderfields", "Order struct fields", "Function"),
        ("cellfun", "Apply to cells", "Function"),
        ("cell2mat", "Cell to matrix", "Function"),
        ("num2cell", "Convert to cell", "Function"),
        ("mat2cell", "Convert to cell", "Function"),
        ("cellstr", "Create cell string", "Function"),
        ("deal", "Distribute inputs", "Function"),
        ("nargin", "Input argument count", "Function"),
        ("nargout", "Output argument count", "Function"),
        ("isequal", "Compare equality", "Function"),
        ("isequaln", "NaN-tolerant equality", "Function"),
        ("ischar", "Check if char", "Function"),
        ("islogical", "Check if logical", "Function"),
        ("isnumeric", "Check if numeric", "Function"),
        ("isreal", "Check if real", "Function"),
        ("isscalar", "Check if scalar", "Function"),
        ("isvector", "Check if vector", "Function"),
        ("isrow", "Check if row", "Function"),
        ("iscolumn", "Check if column", "Function"),
        ("ismatrix", "Check if matrix", "Function"),
        ("isempty", "Check if empty", "Function"),
        ("iscell", "Check if cell", "Function"),
        ("isstruct", "Check if struct", "Function"),
        ("isinf", "Check if infinite", "Function"),
        ("isnan", "Check if NaN", "Function"),
        ("isfinite", "Check if finite", "Function"),
        ("double", "Convert to double", "Function"),
        ("single", "Convert to single", "Function"),
        ("char", "Convert to char", "Function"),
        ("logical", "Convert to logical", "Function"),
        ("pwd", "Current directory", "Function"),
        ("ls", "List directory", "Function"),
        ("dir", "List directory", "Function"),
        ("figure", "Create figure (stub)", "Graphics"),
        ("plot", "Line plot (stub)", "Graphics"),
        ("hold", "Hold plot (stub)", "Graphics"),
        ("subplot", "Subplot (stub)", "Graphics"),
        ("xlabel", "X-axis label (stub)", "Graphics"),
        ("ylabel", "Y-axis label (stub)", "Graphics"),
        ("title", "Plot title (stub)", "Graphics"),
        ("grid", "Grid lines (stub)", "Graphics"),
        ("legend", "Legend (stub)", "Graphics"),
        ("axis", "Axis limits (stub)", "Graphics"),
        ("bar", "Bar chart (stub)", "Graphics"),
        ("histogram", "Histogram (stub)", "Graphics"),
        ("scatter", "Scatter plot (stub)", "Graphics"),
    ];
    for (name, detail, kind) in builtins {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(3), // Function
            detail: Some(format!("{} — {}", kind, detail)),
            insert_text: Some(name.to_string()),
        });
    }
    // Add language keywords
    for kw in &["if", "else", "elseif", "for", "while", "switch", "case",
                 "otherwise", "try", "catch", "function", "return",
                 "break", "continue", "end"] {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(14), // Keyword
            detail: Some("keyword".into()),
            insert_text: Some(kw.to_string()),
        });
    }
    items
}

fn builtin_doc(name: &str) -> String {
    let docs = vec![
        ("sin", "`sin(x)` — Sine of x in radians\n\nElement-wise trigonometric sine."),
        ("cos", "`cos(x)` — Cosine of x in radians"),
        ("tan", "`tan(x)` — Tangent of x in radians"),
        ("exp", "`exp(x)` — Exponential e^x"),
        ("log", "`log(x)` — Natural logarithm"),
        ("sqrt", "`sqrt(x)` — Square root"),
        ("abs", "`abs(x)` — Absolute value / complex magnitude"),
        ("sum", "`sum(x)` or `sum(x, dim)` — Sum of elements"),
        ("mean", "`mean(x)` or `mean(x, dim)` — Mean of elements"),
        ("find", "`find(x)` — Find indices of nonzero elements\n\nReturns column vector of 1-indexed linear indices."),
        ("size", "`size(x)` or `size(x, dim)` — Array dimensions"),
        ("length", "`length(x)` — Length of largest dimension"),
        ("numel", "`numel(x)` — Number of elements"),
        ("zeros", "`zeros(m, n)` — Create matrix of zeros"),
        ("ones", "`ones(m, n)` — Create matrix of ones"),
        ("eye", "`eye(n)` — Identity matrix"),
        ("rand", "`rand(m, n)` — Uniform random numbers [0,1)"),
        ("inv", "`inv(A)` — Matrix inverse"),
        ("det", "`det(A)` — Matrix determinant"),
        ("eig", "`eig(A)` — Eigenvalues and eigenvectors"),
        ("svd", "`svd(A)` — Singular value decomposition"),
        ("qr", "`qr(A)` — QR decomposition"),
        ("lu", "`lu(A)` — LU decomposition"),
        ("chol", "`chol(A)` — Cholesky decomposition"),
        ("disp", "`disp(x)` — Display value"),
        ("sprintf", "`sprintf(fmt, ...)` — Format string\n\nSupported specifiers: `%d`, `%f`, `%g`, `%e`, `%s`"),
        ("strcmp", "`strcmp(s1, s2)` — Compare strings (case-sensitive)"),
        ("upper", "`upper(s)` — Convert to uppercase"),
        ("lower", "`lower(s)` — Convert to lowercase"),
        ("struct", "`struct('field1', val1, ...)` — Create structure array"),
        ("cellfun", "`cellfun(func, C)` — Apply function to each cell"),
        ("deal", "`[a,b,c] = deal(x,y,z)` — Distribute inputs to outputs"),
        ("clear", "`clear` or `clear vars...` — Clear workspace variables"),
        ("who", "`who` — List workspace variables"),
        ("run", "`run('file')` — Execute script file"),
        ("error", "`error(msg)` — Throw an error"),
        ("help", "`help name` — Display help for a function"),
        ("nargin", "`nargin` or `nargin('func')` — Number of input arguments"),
        ("nargout", "`nargout` or `nargout('func')` — Number of output arguments"),
    ];
    for (n, doc) in docs {
        if n == name {
            return format!("**MATBOX builtin**\n---\n{}", doc);
        }
    }
    format!("`{}` — MATBOX identifier", name)
}

// ── Wire protocol helpers ───────────────────────────────────────────────

fn send_response(id: u64, result: Option<serde_json::Value>, error: Option<JsonRpcError>, stdout: &mut io::Stdout) {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result,
        error,
    };
    let body = serde_json::to_string(&resp).unwrap();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = stdout.write_all(header.as_bytes());
    let _ = stdout.write_all(body.as_bytes());
    let _ = stdout.flush();
}

fn send_notification<T: Serialize>(method: &str, params: &T, stdout: &mut io::Stdout) {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: method.into(),
        params: serde_json::to_value(params).unwrap(),
    };
    let body = serde_json::to_string(&notif).unwrap();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = stdout.write_all(header.as_bytes());
    let _ = stdout.write_all(body.as_bytes());
    let _ = stdout.flush();
}
