//! JEIExporter → recipes.json converter, rewritten in Rust.
//!
//! The JEI dumps differ between versions and mods, so the reader is deliberately forgiving: it
//! only looks at fields it recognises, cleans the ids, merges duplicate ingredients and keeps the
//! first recipe it sees for an item (crafting before smelting before everything else).
//!
//! `--selftest` builds a small dump in a temp folder, converts it and checks the result, so CI
//! exercises the real parser instead of only compiling it.

use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Ingredient {
    item: String,
    count: i64,
}

/// Recipes in discovery order, with the lowercased item name as the identity.
struct Recipes {
    order: Vec<(String, Vec<Ingredient>)>,
    seen: HashSet<String>,
}

impl Recipes {
    fn new() -> Self {
        Self { order: Vec::new(), seen: HashSet::new() }
    }

    fn contains(&self, name: &str) -> bool {
        self.seen.contains(&name.to_lowercase())
    }

    fn insert(&mut self, name: String, ingredients: Vec<Ingredient>) {
        self.seen.insert(name.to_lowercase());
        self.order.push((name, ingredients));
    }

    fn to_json(&self) -> Value {
        let mut root = Map::new();
        for (name, ingredients) in &self.order {
            let list: Vec<Value> = ingredients
                .iter()
                .map(|ingredient| json!({ "Item": ingredient.item, "Count": ingredient.count }))
                .collect();
            root.insert(name.clone(), json!({ "Ingredients": list }));
        }
        Value::Object(root)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--selftest") {
        std::process::exit(selftest());
    }

    println!("=== JEIExporter → recipes.json Converter (v2 - smarter) ===");
    println!("Voor Project Ozone 3 / 1.12.2 dumps\n");

    let input_dir = match args.first() {
        Some(dir) => dir.clone(),
        None => {
            print!("Pad naar de map met JEIExporter dump (bijv. exports of jei_exports): ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line.trim().to_string()
        }
    };

    if input_dir.trim().is_empty() || !Path::new(&input_dir).is_dir() {
        println!("Fout: map bestaat niet.");
        return;
    }

    let output_file = match args.get(1) {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("recipes.json"),
    };

    println!("\nInput map : {input_dir}");
    println!("Output    : {}\n", output_file.display());

    let name_map = load_name_map(Path::new(&input_dir));
    let (recipes, added_files) = convert_all(Path::new(&input_dir), &name_map);

    let text = serde_json::to_string_pretty(&recipes.to_json()).unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = std::fs::write(&output_file, text) {
        println!("Fout bij schrijven: {e}");
        return;
    }

    println!("\n========================================");
    println!("Klaar! {} unieke items met recipes", recipes.order.len());
    println!("Totaal toegevoegd: {added_files}");
    println!("Opgeslagen als: {}", output_file.display());
    println!("========================================");
    println!("\nJe kunt dit bestand nu in de Recipe Tree app gebruiken.");
    println!("Tip: open recipes.json en check of de belangrijkste items erin staan.");
}

/// Reads every recipe file, in the same priority order the .NET build used, and reports how many
/// recipes were added per file.
fn convert_all(input_dir: &Path, name_map: &HashMap<String, String>) -> (Recipes, usize) {
    let mut recipes_dir = input_dir.to_path_buf();
    let nested = input_dir.join("recipes");
    if nested.is_dir() {
        recipes_dir = nested;
        println!("Gevonden: recipes/ submap → gebruik die");
    }

    let mut files: Vec<PathBuf> = Vec::new();
    collect_json_files(&recipes_dir, &mut files);
    files.retain(|file| {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        name != "namemap.json" && name != "itemmap.json"
    });
    files.sort_by_key(|file| priority(&file.file_name().and_then(|n| n.to_str()).unwrap_or("")));

    println!("Gevonden JSON bestanden: {}\n", files.len());

    let mut recipes = Recipes::new();
    let mut total_added = 0usize;

    for file in &files {
        let short = file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let content = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) => {
                println!("  ✗ {short} overgeslagen ({e})");
                continue;
            }
        };
        let node: Value = match serde_json::from_str(&content) {
            Ok(node) => node,
            Err(e) => {
                println!("  ✗ {short} overgeslagen ({e})");
                continue;
            }
        };

        let added = process_node(&node, &mut recipes, name_map);
        total_added += added;
        if added > 0 {
            println!("  ✓ {short:<45} → +{added} recipes");
        } else {
            println!("  · {short:<45} (geen bruikbare recipes)");
        }
    }

    (recipes, total_added)
}

fn collect_json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("json")).unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Crafting and furnace first, then the machine categories, then everything else.
fn priority(filename: &str) -> u8 {
    let name = filename.to_lowercase();
    if ["crafting", "workbench", "shaped", "shapeless"].iter().any(|needle| name.contains(needle)) {
        return 0;
    }
    if ["furnace", "smelting", "smelt"].iter().any(|needle| name.contains(needle)) {
        return 1;
    }
    if ["alloy", "infuser", "crusher"].iter().any(|needle| name.contains(needle)) {
        return 2;
    }
    10
}

fn load_name_map(dir: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let candidates = [
        dir.join("nameMap.json"),
        dir.join("namemap.json"),
        dir.join("itemMap.json"),
        dir.join("recipes").join("nameMap.json"),
    ];

    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&text) else { continue };

        for (key, value) in obj {
            map.insert(key.to_lowercase(), plain_text(&value));
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        println!("✓ nameMap geladen ({} entries) uit {name}", map.len());
        break;
    }

    if map.is_empty() {
        println!("· Geen nameMap gevonden (geen probleem, we gebruiken raw ids)");
    }

    map
}

fn process_node(node: &Value, recipes: &mut Recipes, name_map: &HashMap<String, String>) -> usize {
    match node {
        Value::Array(items) => items
            .iter()
            .filter(|item| try_extract_recipe(item, recipes, name_map))
            .count(),
        Value::Object(obj) => {
            // Sometimes everything sits under "recipes".
            if let Some(Value::Array(items)) = obj.get("recipes") {
                return items
                    .iter()
                    .filter(|item| try_extract_recipe(item, recipes, name_map))
                    .count();
            }

            // Otherwise it is one big object of category → layouts / list.
            let mut added = 0;
            for value in obj.values() {
                match value {
                    Value::Array(items) => {
                        added += items
                            .iter()
                            .filter(|item| try_extract_recipe(item, recipes, name_map))
                            .count();
                    }
                    Value::Object(_) => {
                        if try_extract_recipe(value, recipes, name_map) {
                            added += 1;
                        }
                    }
                    _ => {}
                }
            }
            added
        }
        _ => 0,
    }
}

fn try_extract_recipe(node: &Value, recipes: &mut Recipes, name_map: &HashMap<String, String>) -> bool {
    let Value::Object(obj) = node else { return false };

    // ---- output ----
    let mut output = ["output", "result", "outputs", "out", "product"]
        .iter()
        .find_map(|key| obj.get(*key).and_then(get_item_id));

    if output.is_none() {
        for key in ["output", "outputs"] {
            if let Some(Value::Array(items)) = obj.get(key) {
                if let Some(first) = items.first() {
                    output = get_item_id(first);
                }
            }
        }
    }

    if output.is_none() {
        if let Some(Value::Array(slots)) = obj.get("slots") {
            for slot in slots {
                if let Value::Object(slot) = slot {
                    if is_output_slot(slot) {
                        let slot_value = Value::Object(slot.clone());
                        output = slot
                            .get("item")
                            .or_else(|| slot.get("ingredient"))
                            .and_then(get_item_id)
                            .or_else(|| get_item_id(&slot_value));
                        if output.is_some() {
                            break;
                        }
                    }
                }
            }
        }
    }

    let Some(output) = output.filter(|id| !id.trim().is_empty()) else { return false };
    let nice_name = clean_name(&output, name_map);

    // ---- ingredients ----
    let mut ingredients: Vec<Ingredient> = Vec::new();

    // 1. slots array (common in JEI layouts)
    if let Some(Value::Array(slots)) = obj.get("slots") {
        for slot in slots {
            let Value::Object(slot) = slot else { continue };
            if is_output_slot(slot) {
                continue;
            }
            let slot_value = Value::Object(slot.clone());
            let id = slot
                .get("item")
                .or_else(|| slot.get("ingredient"))
                .and_then(get_item_id)
                .or_else(|| get_item_id(&slot_value));
            if let Some(id) = id.filter(|id| !id.trim().is_empty()) {
                ingredients.push(Ingredient {
                    item: clean_name(&id, name_map),
                    count: get_count(&slot_value),
                });
            }
        }
    }

    // 2. inputs / ingredients / input / catalysts
    for key in ["inputs", "ingredients", "input", "catalysts"] {
        match obj.get(key) {
            Some(Value::Array(items)) => {
                for item in items {
                    if let Some(id) = get_item_id(item).filter(|id| !id.trim().is_empty()) {
                        ingredients.push(Ingredient {
                            item: clean_name(&id, name_map),
                            count: get_count(item),
                        });
                    }
                }
            }
            Some(value @ Value::Object(_)) => {
                if let Some(id) = get_item_id(value).filter(|id| !id.trim().is_empty()) {
                    ingredients.push(Ingredient {
                        item: clean_name(&id, name_map),
                        count: get_count(value),
                    });
                }
            }
            _ => {}
        }
    }

    if ingredients.is_empty() {
        return false;
    }

    // Merge duplicates (case-insensitive), summing their counts.
    let mut merged: Vec<Ingredient> = Vec::new();
    for ingredient in ingredients {
        if ingredient.item.trim().is_empty() {
            continue;
        }
        match merged
            .iter_mut()
            .find(|existing| existing.item.eq_ignore_ascii_case(&ingredient.item))
        {
            Some(existing) => existing.count += ingredient.count,
            None => merged.push(ingredient),
        }
    }

    if merged.is_empty() {
        return false;
    }

    // Only add an item when it is not known yet: the first (best) source wins.
    if recipes.contains(&nice_name) {
        return false;
    }
    recipes.insert(nice_name, merged);
    true
}

fn get_item_id(node: &Value) -> Option<String> {
    match node {
        Value::Null => None,
        Value::String(text) => {
            if text.trim().is_empty() || text == "null" {
                None
            } else {
                Some(clean_raw_id(text))
            }
        }
        Value::Number(number) => Some(clean_raw_id(&number.to_string())),
        Value::Bool(flag) => Some(clean_raw_id(&flag.to_string())),
        Value::Object(obj) => {
            for key in ["item", "id", "name", "registry_name", "registryName", "ingredient", "stack"] {
                if let Some(value) = obj.get(key) {
                    if value.is_null() {
                        continue;
                    }
                    let text = plain_text(value);
                    if !text.trim().is_empty() && text != "null" {
                        return Some(clean_raw_id(&text));
                    }
                }
            }

            if let Some(Value::Array(stacks)) = obj.get("stacks") {
                if let Some(first) = stacks.first() {
                    return get_item_id(first);
                }
            }
            if let Some(value) = obj.get("value") {
                return get_item_id(value);
            }
            if let Some(value) = obj.get("content") {
                return get_item_id(value);
            }
            None
        }
        Value::Array(_) => None,
    }
}

fn get_count(node: &Value) -> i64 {
    if let Value::Object(obj) = node {
        for key in ["count", "amount", "Count"] {
            if let Some(value) = obj.get(key) {
                if let Ok(count) = plain_text(value).trim().parse::<i64>() {
                    if count > 0 {
                        return count;
                    }
                }
            }
        }
    }
    1
}

fn is_output_slot(slot: &Map<String, Value>) -> bool {
    let role = ["role", "type", "kind"]
        .iter()
        .find_map(|key| slot.get(*key))
        .map(plain_text)
        .unwrap_or_default()
        .to_lowercase();
    role.contains("output") || role.contains("result") || role.contains("product")
}

/// A JSON string as-is; anything else as its compact JSON text (what `JsonNode.ToString()` gave).
fn plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn clean_raw_id(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == ' ' || c == '\t');
    let without_nbt = match trimmed.find('{') {
        Some(index) if index > 0 => &trimmed[..index],
        _ => trimmed,
    };
    let lower = without_nbt.to_lowercase();
    let without_prefix = if lower.starts_with("item:") { &without_nbt[5..] } else { without_nbt };
    without_prefix.trim().to_string()
}

fn clean_name(id: &str, name_map: &HashMap<String, String>) -> String {
    if let Some(nice) = name_map.get(&id.to_lowercase()) {
        if !nice.trim().is_empty() {
            return sanitize(nice);
        }
    }

    let tail = id.rsplit(':').next().unwrap_or(id);
    sanitize(tail)
}

fn sanitize(name: &str) -> String {
    static NOT_ALLOWED: OnceLock<Regex> = OnceLock::new();
    static REPEATED: OnceLock<Regex> = OnceLock::new();

    let lower = name.to_lowercase();
    let replaced = NOT_ALLOWED.get_or_init(|| Regex::new(r"[^a-z0-9_]").unwrap()).replace_all(&lower, "_");
    let collapsed = REPEATED.get_or_init(|| Regex::new(r"_+").unwrap()).replace_all(&replaced, "_");
    let trimmed = collapsed.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Builds a tiny dump, converts it and checks what came out.
fn selftest() -> i32 {
    let mut checks: Vec<(bool, String)> = Vec::new();
    macro_rules! check {
        ($cond:expr, $name:expr) => {
            checks.push(($cond, $name.to_string()))
        };
    }

    check!(sanitize("Iron Ingot") == "iron_ingot", "sanitize spaties");
    check!(sanitize("__Weird--Name!!") == "weird_name", "sanitize leestekens");
    check!(sanitize("") == "unknown", "sanitize leeg");
    check!(clean_raw_id("minecraft:iron_ingot{display:{Name:\"x\"}}") == "minecraft:iron_ingot", "NBT strippen");
    check!(clean_raw_id("item:minecraft:stone") == "minecraft:stone", "item:-prefix strippen");
    check!(priority("crafting_shaped.json") == 0, "prioriteit crafting");
    check!(priority("furnace.json") == 1, "prioriteit furnace");
    check!(priority("alloy_smelter.json") == 1, "prioriteit smelting");
    check!(priority("random.json") == 10, "prioriteit overig");

    let dir = std::env::temp_dir().join(format!("jeiconverter-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let recipes_dir = dir.join("recipes");
    std::fs::create_dir_all(&recipes_dir).unwrap();

    std::fs::write(
        dir.join("nameMap.json"),
        r#"{"minecraft:iron_ingot":"Iron Ingot","minecraft:iron_ore":"Iron Ore"}"#,
    )
    .unwrap();
    std::fs::write(
        recipes_dir.join("crafting_shaped.json"),
        r#"{"recipes":[
            {"output":{"item":"minecraft:iron_ingot"},"inputs":[{"item":"minecraft:iron_ore","count":2},{"item":"minecraft:iron_ore","count":1}]},
            {"output":{"item":"minecraft:iron_ingot"},"inputs":[{"item":"minecraft:stone"}]},
            {"output":{"item":"minecraft:stick"},"inputs":[]}
        ]}"#,
    )
    .unwrap();
    std::fs::write(
        recipes_dir.join("furnace.json"),
        r#"[{"output":"minecraft:gold_ingot","input":{"item":"minecraft:gold_ore","count":4}}]"#,
    )
    .unwrap();
    // Lower priority and the same output as the crafting recipe: it must not replace it.
    std::fs::write(
        recipes_dir.join("sawmill.json"),
        r#"[{"output":"minecraft:iron_ingot","ingredients":[{"item":"minecraft:stone","count":7}]}]"#,
    )
    .unwrap();

    let name_map = load_name_map(&dir);
    check!(name_map.len() == 2, "nameMap geladen");

    let (recipes, added) = convert_all(&dir, &name_map);
    let json = recipes.to_json();
    check!(added == 2, "twee recipes toegevoegd");
    check!(recipes.order.len() == 2, "twee unieke items");
    check!(json.get("iron_ingot").is_some(), "iron_ingot in uitvoer");
    check!(json.get("gold_ingot").is_some(), "gold_ingot in uitvoer");
    check!(recipes.order.first().map(|(name, _)| name.as_str()) == Some("iron_ingot"), "eerste bron wint");
    check!(json.get("stick").is_none(), "recipe zonder ingredienten overgeslagen");

    let ingredient_count = json
        .get("iron_ingot")
        .and_then(|value| value.get("Ingredients"))
        .and_then(|value| value.as_array())
        .map(|list| list.len())
        .unwrap_or(0);
    check!(ingredient_count == 1, "dubbele ingredienten samengevoegd");
    let merged_count = json
        .get("iron_ingot")
        .and_then(|value| value.get("Ingredients"))
        .and_then(|value| value.as_array())
        .and_then(|list| list.first())
        .and_then(|value| value.get("Count"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    check!(merged_count == 3, "aantallen opgeteld");

    let written = dir.join("recipes.json");
    std::fs::write(&written, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    let reread: Value = serde_json::from_str(&std::fs::read_to_string(&written).unwrap()).unwrap();
    check!(
        reread.get("iron_ingot").and_then(|v| v.get("Ingredients")).is_some(),
        "uitvoer is geldige json met de juiste vorm"
    );

    let _ = std::fs::remove_dir_all(&dir);

    let failed = checks.iter().filter(|(ok, _)| !ok).count();
    for (ok, name) in &checks {
        println!("{} {name}", if *ok { "✔" } else { "✖" });
    }
    if failed == 0 {
        println!("✔ alles goed: {} controle(s) geslaagd", checks.len());
        0
    } else {
        println!("✖ {failed} van {} controle(s) mislukt", checks.len());
        1
    }
}
