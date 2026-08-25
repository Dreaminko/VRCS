const SUPPORTED_TRANSLATION_PROMPT_VARIABLES = new Set([
  "source_language",
  "target_language",
  "glossary",
  "context",
]);

export function translationPromptVariableError(template: string): string | null {
  let variableStart: number | null = null;

  for (let index = 0; index < template.length; index += 1) {
    const character = template[index];
    if (character === "{") {
      if (variableStart !== null) {
        return "Translation system prompt contains a nested variable";
      }
      variableStart = index + 1;
      continue;
    }
    if (character !== "}") continue;
    if (variableStart === null) {
      return "Translation system prompt contains an unmatched closing brace";
    }

    const variable = template.slice(variableStart, index);
    if (!SUPPORTED_TRANSLATION_PROMPT_VARIABLES.has(variable)) {
      return `Unsupported translation prompt variable: {${variable}}`;
    }
    variableStart = null;
  }

  return variableStart === null
    ? null
    : "Translation system prompt contains an unclosed variable";
}
