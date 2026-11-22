using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace JEIConverter;

class Program
{
    static void Main(string[] args)
    {
        Console.WriteLine("=== JEIExporter → recipes.json Converter (v2 - smarter) ===");
        Console.WriteLine("Voor Project Ozone 3 / 1.12.2 dumps\n");

        string inputDir;
        string outputFile;

        if (args.Length >= 1)
            inputDir = args[0];
        else
        {
            Console.Write("Pad naar de map met JEIExporter dump (bijv. exports of jei_exports): ");
            inputDir = Console.ReadLine()?.Trim() ?? "";
        }

        if (string.IsNullOrWhiteSpace(inputDir) || !Directory.Exists(inputDir))
        {
            Console.WriteLine("Fout: map bestaat niet.");
            return;
        }

        outputFile = args.Length >= 2
            ? args[1]
            : Path.Combine(Directory.GetCurrentDirectory(), "recipes.json");

        Console.WriteLine($"\nInput map : {inputDir}");
        Console.WriteLine($"Output    : {outputFile}\n");

        var recipes = new Dictionary<string, Recipe>(StringComparer.OrdinalIgnoreCase);

        // 1. nameMap laden (voor nette namen)
        var nameMap = LoadNameMap(inputDir);

        // 2. Bepaal waar de recipe-bestanden staan
        // Typische JEIExporter layout: exports/nameMap.json + exports/recipes/*.json
        string recipesDir = inputDir;
        string possibleRecipesDir = Path.Combine(inputDir, "recipes");
        if (Directory.Exists(possibleRecipesDir))
        {
            recipesDir = possibleRecipesDir;
            Console.WriteLine("Gevonden: recipes/ submap → gebruik die");
        }

        var jsonFiles = Directory.GetFiles(recipesDir, "*.json", SearchOption.AllDirectories)
                                 .Where(f => !Path.GetFileName(f).Equals("nameMap.json", StringComparison.OrdinalIgnoreCase)
                                          && !Path.GetFileName(f).Equals("namemap.json", StringComparison.OrdinalIgnoreCase))
                                 .OrderBy(f => Priority(Path.GetFileName(f))) // crafting/smelting eerst
                                 .ToList();

        Console.WriteLine($"Gevonden JSON bestanden: {jsonFiles.Count}\n");

        int totalAdded = 0;

        foreach (var file in jsonFiles)
        {
            try
            {
                string content = File.ReadAllText(file);
                var node = JsonNode.Parse(content);
                if (node == null) continue;

                int added = ProcessNode(node, recipes, nameMap);
                totalAdded += added;

                string shortName = Path.GetFileName(file);
                if (added > 0)
                    Console.WriteLine($"  ✓ {shortName,-45} → +{added} recipes");
                else
                    Console.WriteLine($"  · {shortName,-45} (geen bruikbare recipes)");
            }
            catch (Exception ex)
            {
                Console.WriteLine($"  ✗ {Path.GetFileName(file)} overgeslagen ({ex.Message})");
            }
        }

        // 3. Schrijf resultaat
        var options = new JsonSerializerOptions
        {
            WriteIndented = true
        };

        string jsonOut = JsonSerializer.Serialize(recipes, options);
        File.WriteAllText(outputFile, jsonOut);

        Console.WriteLine($"\n========================================");
        Console.WriteLine($"Klaar! {recipes.Count} unieke items met recipes");
        Console.WriteLine($"Totaal toegevoegd: {totalAdded}");
        Console.WriteLine($"Opgeslagen als: {outputFile}");
        Console.WriteLine("========================================");
        Console.WriteLine("\nJe kunt dit bestand nu in de Recipe Tree app gebruiken.");
        Console.WriteLine("Tip: open recipes.json en check of de belangrijkste items erin staan.");
    }

    // Prioriteit: crafting en furnace eerst, dan de rest
    static int Priority(string filename)
    {
        filename = filename.ToLowerInvariant();
        if (filename.Contains("crafting") || filename.Contains("workbench") || filename.Contains("shaped") || filename.Contains("shapeless"))
            return 0;
        if (filename.Contains("furnace") || filename.Contains("smelting") || filename.Contains("smelt"))
            return 1;
        if (filename.Contains("alloy") || filename.Contains("infuser") || filename.Contains("crusher"))
            return 2;
        return 10;
    }

    static Dictionary<string, string> LoadNameMap(string dir)
    {
        var map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

        string[] possible = {
            Path.Combine(dir, "nameMap.json"),
            Path.Combine(dir, "namemap.json"),
            Path.Combine(dir, "itemMap.json"),
            Path.Combine(dir, "recipes", "nameMap.json")
        };

        foreach (var path in possible)
        {
            if (!File.Exists(path)) continue;

            try
            {
                var node = JsonNode.Parse(File.ReadAllText(path));
                if (node is JsonObject obj)
                {
                    foreach (var kv in obj)
                    {
                        if (kv.Value != null)
                            map[kv.Key] = kv.Value.ToString() ?? kv.Key;
                    }
                }
                Console.WriteLine($"✓ nameMap geladen ({map.Count} entries) uit {Path.GetFileName(path)}");
                break;
            }
            catch { }
        }

        if (map.Count == 0)
            Console.WriteLine("· Geen nameMap gevonden (geen probleem, we gebruiken raw ids)");

        return map;
    }

    static int ProcessNode(JsonNode node, Dictionary<string, Recipe> recipes, Dictionary<string, string> nameMap)
    {
        int added = 0;

        if (node is JsonArray arr)
        {
            foreach (var item in arr)
                if (item != null)
                    added += TryExtractRecipe(item, recipes, nameMap);
            return added;
        }

        if (node is JsonObject obj)
        {
            // Soms zit alles onder "recipes"
            if (obj["recipes"] is JsonArray recipesArr)
            {
                foreach (var item in recipesArr)
                    if (item != null)
                        added += TryExtractRecipe(item, recipes, nameMap);
                return added;
            }

            // Soms is het één groot object met category → layouts / list
            foreach (var kv in obj)
            {
                if (kv.Value is JsonArray subArr)
                {
                    foreach (var item in subArr)
                        if (item != null)
                            added += TryExtractRecipe(item, recipes, nameMap);
                }
                else if (kv.Value is JsonObject subObj)
                {
                    added += TryExtractRecipe(subObj, recipes, nameMap);
                }
            }
        }

        return added;
    }

    static int TryExtractRecipe(JsonNode node, Dictionary<string, Recipe> recipes, Dictionary<string, string> nameMap)
    {
        if (node is not JsonObject obj) return 0;

        // === OUTPUT vinden ===
        string? outputItem = null;

        // Meest voorkomende velden
        outputItem = GetItemId(obj["output"] ?? obj["result"] ?? obj["outputs"] ?? obj["out"] ?? obj["product"]);

        // Soms is output een array
        if (outputItem == null && obj["output"] is JsonArray outArr && outArr.Count > 0)
            outputItem = GetItemId(outArr[0]);
        if (outputItem == null && obj["outputs"] is JsonArray outsArr && outsArr.Count > 0)
            outputItem = GetItemId(outsArr[0]);

        // Soms zit output in slots met role=output
        if (outputItem == null && obj["slots"] is JsonArray slots)
        {
            foreach (var slot in slots)
            {
                if (slot is JsonObject slotObj && IsOutputSlot(slotObj))
                {
                    outputItem = GetItemId(slotObj["item"] ?? slotObj["ingredient"] ?? slotObj);
                    if (outputItem != null) break;
                }
            }
        }

        if (string.IsNullOrWhiteSpace(outputItem)) return 0;

        string niceName = CleanName(outputItem, nameMap);

        // === INGREDIENTS verzamelen ===
        var ingredients = new List<Ingredient>();

        // 1. slots array (vaak in JEI layouts)
        if (obj["slots"] is JsonArray slotArr)
        {
            foreach (var slot in slotArr)
            {
                if (slot is not JsonObject slotObj) continue;
                if (IsOutputSlot(slotObj)) continue;

                string? id = GetItemId(slotObj["item"] ?? slotObj["ingredient"] ?? slotObj);
                if (!string.IsNullOrWhiteSpace(id))
                {
                    ingredients.Add(new Ingredient
                    {
                        Item = CleanName(id, nameMap),
                        Count = GetCount(slotObj)
                    });
                }
            }
        }

        // 2. inputs / ingredients / input velden
        var inputNodes = new[] { obj["inputs"], obj["ingredients"], obj["input"], obj["catalysts"] };
        foreach (var inputNode in inputNodes)
        {
            if (inputNode is JsonArray inArr)
            {
                foreach (var ing in inArr)
                {
                    string? id = GetItemId(ing);
                    if (!string.IsNullOrWhiteSpace(id))
                    {
                        ingredients.Add(new Ingredient
                        {
                            Item = CleanName(id, nameMap),
                            Count = GetCount(ing)
                        });
                    }
                }
            }
            else if (inputNode is JsonObject inObj)
            {
                string? id = GetItemId(inObj);
                if (!string.IsNullOrWhiteSpace(id))
                {
                    ingredients.Add(new Ingredient
                    {
                        Item = CleanName(id, nameMap),
                        Count = GetCount(inObj)
                    });
                }
            }
        }

        if (ingredients.Count == 0) return 0;

        // Merge duplicate ingredients
        var merged = ingredients
            .GroupBy(i => i.Item, StringComparer.OrdinalIgnoreCase)
            .Select(g => new Ingredient
            {
                Item = g.Key,
                Count = g.Sum(x => x.Count)
            })
            .Where(i => !string.IsNullOrWhiteSpace(i.Item))
            .ToList();

        if (merged.Count == 0) return 0;

        // Alleen toevoegen als we dit item nog niet hebben (eerste wint)
        if (!recipes.ContainsKey(niceName))
        {
            recipes[niceName] = new Recipe { Ingredients = merged };
            return 1;
        }

        return 0;
    }

    static string? GetItemId(JsonNode? node)
    {
        if (node == null) return null;

        if (node is JsonValue val)
        {
            string s = val.ToString() ?? "";
            if (string.IsNullOrWhiteSpace(s) || s == "null") return null;
            return CleanRawId(s);
        }

        if (node is JsonObject obj)
        {
            // Meest voorkomende velden in JEI dumps
            string[] candidates = { "item", "id", "name", "registry_name", "registryName", "ingredient", "stack" };
            foreach (var c in candidates)
            {
                if (obj[c] != null)
                {
                    string? v = obj[c]?.ToString();
                    if (!string.IsNullOrWhiteSpace(v) && v != "null")
                        return CleanRawId(v);
                }
            }

            // Soms stacks array
            if (obj["stacks"] is JsonArray stacks && stacks.Count > 0)
                return GetItemId(stacks[0]);

            // Soms "value" of "content"
            if (obj["value"] != null) return GetItemId(obj["value"]);
            if (obj["content"] != null) return GetItemId(obj["content"]);
        }

        return null;
    }

    static int GetCount(JsonNode? node)
    {
        if (node is JsonObject obj)
        {
            if (obj["count"] != null && int.TryParse(obj["count"]?.ToString(), out int c) && c > 0)
                return c;
            if (obj["amount"] != null && int.TryParse(obj["amount"]?.ToString(), out int a) && a > 0)
                return a;
            if (obj["Count"] != null && int.TryParse(obj["Count"]?.ToString(), out int C) && C > 0)
                return C;
        }
        return 1;
    }

    static bool IsOutputSlot(JsonObject slot)
    {
        string role = (slot["role"] ?? slot["type"] ?? slot["kind"] ?? "").ToString()?.ToLowerInvariant() ?? "";
        return role.Contains("output") || role.Contains("result") || role.Contains("product");
    }

    static string CleanRawId(string raw)
    {
        raw = raw.Trim().Trim('"', '\'', ' ', '\t');
        // NBT weghalen
        int brace = raw.IndexOf('{');
        if (brace > 0) raw = raw[..brace];
        // Soms komt er "item:" voor
        if (raw.StartsWith("item:", StringComparison.OrdinalIgnoreCase))
            raw = raw[5..];
        return raw.Trim();
    }

    static string CleanName(string id, Dictionary<string, string> nameMap)
    {
        // Eerst proberen via nameMap
        if (nameMap.TryGetValue(id, out var nice) && !string.IsNullOrWhiteSpace(nice))
            return Sanitize(nice);

        // Fallback: deel na de :
        if (id.Contains(':'))
            id = id.Split(':').Last();

        return Sanitize(id);
    }

    static string Sanitize(string name)
    {
        name = name.ToLowerInvariant();
        name = Regex.Replace(name, @"[^a-z0-9_]", "_");
        name = Regex.Replace(name, @"_+", "_").Trim('_');
        return string.IsNullOrEmpty(name) ? "unknown" : name;
    }
}

public class Recipe
{
    public List<Ingredient> Ingredients { get; set; } = new();
}

public class Ingredient
{
    public string Item { get; set; } = "";
    public int Count { get; set; } = 1;
}
