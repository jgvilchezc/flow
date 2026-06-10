// Benchmarks Flow's formatting pass: same endpoint, body and params as
// src-tauri/src/format.rs, sharing the prompt files under src-tauri/prompts/.
//
// Two modes:
//   node scripts/bench_format.mjs [model]            base regression cases
//   node scripts/bench_format.mjs [model] --style    style-fragment matrix
//
// In --style mode we replicate prompt::augment_system_prompt: the base system
// prompt, then a blank line, then the tone fragment. The three fragment strings
// below are copied verbatim from src-tauri/src/prompt.rs (the Personal-context
// arm of style_fragment) — that file is the source of truth; keep these in sync
// if the Rust fragments change.
import { readFileSync } from "node:fs";

const model = process.argv[2] ?? "qwen2.5:7b";
const styleMode = process.argv.includes("--style");
const SYSTEM_PROMPT = readFileSync(
  new URL("../src-tauri/prompts/system_prompt.txt", import.meta.url),
  "utf8",
);
const FEW_SHOT = JSON.parse(
  readFileSync(new URL("../src-tauri/prompts/few_shot.json", import.meta.url), "utf8"),
);

// Copied verbatim from src-tauri/src/prompt.rs style_fragment(tone, Context::Personal).
// SOURCE OF TRUTH: src-tauri/src/prompt.rs — keep in sync.
const STYLE_FRAGMENTS = {
  formal:
    "EN: Write personal messages in a formal register: full capitalization, complete punctuation, no slang. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro formal: mayúsculas completas, puntuación completa, sin jerga. Aplica este registro en el idioma del dictado.",
  casual:
    "EN: Write personal messages in a casual register: keep sentence capitalization but use lighter punctuation; a relaxed, friendly tone. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro casual: conserva las mayúsculas de oración pero usa puntuación ligera; tono relajado y amistoso. Aplica este registro en el idioma del dictado.",
  very_casual:
    "EN: Write personal messages in a very casual register: no leading capitals, minimal punctuation, chat-style. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro muy casual: sin mayúsculas iniciales, puntuación mínima, estilo chat. Aplica este registro en el idioma del dictado.",
};

// Mirrors prompt::augment_system_prompt(base, [], Some(fragment)).
function augment(base, fragment) {
  return `${base}\n\n${fragment}`;
}

// Two informal-speech cases (1 ES, 1 EN) run against every tone so the
// register difference is observable side by side.
const styleCases = [
  ["es", "che mañana paso por tu casa tipo a las ocho y vemos la peli esa que querías"],
  ["en", "hey so im gonna swing by around eight tomorrow and we can watch that movie you wanted"],
];

async function chat(system, transcript) {
  const started = performance.now();
  const res = await fetch("http://localhost:11434/api/chat", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model,
      stream: false,
      options: { temperature: 0.1 },
      messages: [
        { role: "system", content: system },
        ...FEW_SHOT,
        { role: "user", content: transcript },
      ],
    }),
  });
  const body = await res.json();
  const ms = Math.round(performance.now() - started);
  const text = (body.message?.content ?? "<ERROR>").trim();
  return { ms, text };
}

if (styleMode) {
  console.log(`model: ${model}  (style fragment matrix)\n`);
  for (const tone of ["formal", "casual", "very_casual"]) {
    const system = augment(SYSTEM_PROMPT, STYLE_FRAGMENTS[tone]);
    console.log(`══ tone: ${tone}`);
    for (const [lang, transcript] of styleCases) {
      const { ms, text } = await chat(system, transcript);
      console.log(`── ${lang} [${ms} ms]`);
      console.log(`   in : ${transcript}`);
      console.log(`   out: ${text.replaceAll("\n", "\n        ")}`);
    }
    console.log();
  }
  process.exit(0);
}

const cases = [
  ["es-list", "necesito que compres tres cosas eh leche huevos y pan"],
  ["es-corr", "llegamos el viernes no pará mejor el sábado a las diez"],
  [
    "es-desc",
    "este la arquitectura tiene o sea tres capas la de dominio la de aplicación y la de infraestructura",
  ],
  [
    "en-list",
    "ok so um I need three things for the demo a laptop the cable and the projector",
  ],
  ["en-punct", "the meeting is at five pm period don't be late"],
  [
    "es-long",
    "hola buenas tardes quería avisarte que la reunión del lunes se pasa para el miércoles a las tres de la tarde porque el cliente eh pidió más tiempo para revisar la propuesta así que mejor preparate eh los números actualizados el reporte de ventas y la presentación nueva",
  ],
  // regressions: a small model must never answer the transcript or invent content
  ["es-greeting", "hola"],
  ["es-question", "qué de qué estás hablando"],
  ["es-intent", "quiero hacer una aplicación de un agente conversacional agrícola"],
];

console.log(`model: ${model}\n`);
for (const [name, transcript] of cases) {
  const { ms, text } = await chat(SYSTEM_PROMPT, transcript);
  console.log(`── ${name} [${ms} ms]`);
  console.log(`   in : ${transcript}`);
  console.log(`   out: ${text.replaceAll("\n", "\n        ")}`);
  console.log();
}
