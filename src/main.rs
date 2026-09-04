use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::process;

const VALID_TYPES: &[&str] = &["string", "integer", "boolean", "path", "enum"];
const VALID_RISKS: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolArg {
    name: String,
    kind: String,
    required: bool,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tool {
    name: String,
    risk: String,
    description: String,
    args: Vec<ToolArg>,
}

fn unquote(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        Ok(trimmed[1..trimmed.len() - 1].replace("\\\"", "\""))
    } else if trimmed.contains(' ') {
        Err(format!("value with spaces must be quoted: {trimmed}"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn parse(text: &str) -> Result<Vec<Tool>, String> {
    let mut tools: Vec<Tool> = Vec::new();
    let mut names = BTreeSet::new();
    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("tool") => {
                let name = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: tool needs a name"))?;
                let risk = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: tool needs a risk"))?;
                let rest = line.splitn(4, char::is_whitespace).nth(3).unwrap_or("");
                let description = unquote(rest).map_err(|e| format!("line {line_number}: {e}"))?;
                if !VALID_RISKS.contains(&risk) {
                    return Err(format!("line {line_number}: invalid risk '{risk}'"));
                }
                if !names.insert(name.to_string()) {
                    return Err(format!("line {line_number}: duplicate tool '{name}'"));
                }
                tools.push(Tool {
                    name: name.to_string(),
                    risk: risk.to_string(),
                    description,
                    args: Vec::new(),
                });
            }
            Some("arg") => {
                let tool_name = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: arg needs a tool"))?;
                let arg_name = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: arg needs a name"))?;
                let kind = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: arg needs a type"))?;
                let requirement = words
                    .next()
                    .ok_or_else(|| format!("line {line_number}: arg needs required|optional"))?;
                let rest = line.splitn(6, char::is_whitespace).nth(5).unwrap_or("");
                let description = unquote(rest).map_err(|e| format!("line {line_number}: {e}"))?;
                if !VALID_TYPES.contains(&kind) {
                    return Err(format!("line {line_number}: invalid type '{kind}'"));
                }
                let required = match requirement {
                    "required" => true,
                    "optional" => false,
                    other => {
                        return Err(format!("line {line_number}: invalid requirement '{other}'"));
                    }
                };
                let tool = tools
                    .iter_mut()
                    .find(|tool| tool.name == tool_name)
                    .ok_or_else(|| format!("line {line_number}: unknown tool '{tool_name}'"))?;
                if tool.args.iter().any(|arg| arg.name == arg_name) {
                    return Err(format!("line {line_number}: duplicate arg '{arg_name}'"));
                }
                tool.args.push(ToolArg {
                    name: arg_name.to_string(),
                    kind: kind.to_string(),
                    required,
                    description,
                });
            }
            Some(other) => return Err(format!("line {line_number}: unknown directive '{other}'")),
            None => {}
        }
    }
    if tools.is_empty() {
        return Err("contract file has no tools".to_string());
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tools)
}

fn validate(tools: &[Tool]) -> Result<(), String> {
    for tool in tools {
        if tool.args.is_empty() {
            return Err(format!("tool '{}' has no args", tool.name));
        }
        let required = tool.args.iter().filter(|arg| arg.required).count();
        if required == 0 {
            return Err(format!(
                "tool '{}' needs at least one required arg",
                tool.name
            ));
        }
    }
    Ok(())
}

fn load(path: &str) -> Result<Vec<Tool>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let tools = parse(&text)?;
    validate(&tools)?;
    Ok(tools)
}

fn contract(tool: &Tool) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"name\": \"{}\",\n", tool.name));
    out.push_str(&format!("  \"risk\": \"{}\",\n", tool.risk));
    out.push_str(&format!("  \"description\": \"{}\",\n", tool.description));
    out.push_str("  \"args\": [\n");
    for (index, arg) in tool.args.iter().enumerate() {
        let comma = if index + 1 == tool.args.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!(
            "    {{ \"name\": \"{}\", \"type\": \"{}\", \"required\": {}, \"description\": \"{}\" }}{comma}\n",
            arg.name, arg.kind, arg.required, arg.description
        ));
    }
    out.push_str("  ]\n}");
    out
}

fn by_name(tools: &[Tool]) -> BTreeMap<&str, &Tool> {
    tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect()
}

fn usage() {
    eprintln!(
        "toolsmith — validate and render agent tool contracts\n\n\
         FORMAT:\n\
           tool fs low \"Filesystem operations\"\n\
           arg fs path path required \"Path to inspect\"\n\n\
         USAGE:\n\
           toolsmith validate <contracts.tools>\n\
           toolsmith list <contracts.tools>\n\
           toolsmith contract <contracts.tools> <tool-name>"
    );
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("validate") => {
            let path = args.get(1).ok_or("validate requires a contract path")?;
            let tools = load(path)?;
            println!("VALID {} tools in {path}", tools.len());
            Ok(())
        }
        Some("list") => {
            let path = args.get(1).ok_or("list requires a contract path")?;
            for tool in load(path)? {
                println!(
                    "{} [{}] — {} ({} args)",
                    tool.name,
                    tool.risk,
                    tool.description,
                    tool.args.len()
                );
            }
            Ok(())
        }
        Some("contract") => {
            let path = args.get(1).ok_or("contract requires a contract path")?;
            let name = args.get(2).ok_or("contract requires a tool name")?;
            let tools = load(path)?;
            let tools_by_name = by_name(&tools);
            let tool = tools_by_name
                .get(name.as_str())
                .ok_or_else(|| format!("unknown tool '{name}'"))?;
            println!("{}", contract(tool));
            Ok(())
        }
        Some("--help") | Some("-h") | None => {
            usage();
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other}; try --help")),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("toolsmith: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "tool fs low \"Filesystem operations\"\narg fs path path required \"Path to inspect\"\ntool git medium \"Git operations\"\narg git repo path required \"Repository path\"\narg git dry_run boolean optional \"Preview only\"\n";

    #[test]
    fn parses_valid_contract() {
        let tools = parse(GOOD).unwrap();
        validate(&tools).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "fs");
        assert!(tools[1].args[1].required == false);
    }

    #[test]
    fn rejects_unknown_tool_reference() {
        let error = parse("arg nope path string required \"Path\"").unwrap_err();
        assert!(error.contains("unknown tool"));
    }

    #[test]
    fn rejects_duplicate_tools() {
        let bad = "tool fs low \"one\"\ntool fs low \"two\"";
        assert!(parse(bad).unwrap_err().contains("duplicate tool"));
    }

    #[test]
    fn renders_contract_shape() {
        let tools = parse(GOOD).unwrap();
        let rendered = contract(&tools[0]);
        assert!(rendered.contains("\"name\": \"fs\""));
        assert!(rendered.contains("\"required\": true"));
    }
}
