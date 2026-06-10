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

// Mirrors src-tauri/src/prompt.rs: style_fragment(tone, Context::Personal)
// (imperative override + EN/ES examples, composed exactly like the
// style_override! macro) and style_shots(tone) (the same examples as real
// user/assistant turns). SOURCE OF TRUTH: src-tauri/src/prompt.rs — keep in sync.
const STYLE_SHOT_INPUTS = [
  "hey are you free for lunch tomorrow lets do twelve if that works",
  "dale nos vemos mañana tipo a las ocho en casa",
];
const STYLE_SHOTS = {
  formal: [
    [STYLE_SHOT_INPUTS[0], "Hey, are you free for lunch tomorrow? Let's do 12 if that works."],
    [STYLE_SHOT_INPUTS[1], "Dale, nos vemos mañana tipo a las 8 en casa."],
  ],
  casual: [
    [STYLE_SHOT_INPUTS[0], "Hey are you free for lunch tomorrow? Let's do 12 if that works"],
    [STYLE_SHOT_INPUTS[1], "Dale nos vemos mañana tipo a las 8 en casa"],
  ],
  very_casual: [
    [STYLE_SHOT_INPUTS[0], "hey are you free for lunch tomorrow? let's do 12 if that works"],
    [STYLE_SHOT_INPUTS[1], "dale nos vemos mañana tipo a las 8 en casa"],
  ],
};
function styleOverride(tone, rules, exEn, exEs) {
  return (
    `STYLE OVERRIDE — ${tone} register for personal messages. When any rule above conflicts with this override, THIS OVERRIDE WINS. ${rules} Apply the register in the transcript's own language (Spanish stays Spanish, English stays English). Never change the speaker's words — adjust ONLY capitalization and punctuation.\n` +
    `Style example: "hey are you free for lunch tomorrow lets do twelve if that works" -> "${exEn}"\n` +
    `Style example: "dale nos vemos mañana tipo a las ocho en casa" -> "${exEs}"`
  );
}

const STYLE_FRAGMENTS = {
  formal: styleOverride(
    "formal",
    "Capitalize every sentence and use complete punctuation: natural commas, question marks, and a final period on every sentence.",
    "Hey, are you free for lunch tomorrow? Let's do 12 if that works.",
    "Dale, nos vemos mañana tipo a las 8 en casa.",
  ),
  casual: styleOverride(
    "casual",
    "Keep sentence capitalization, but lighten punctuation: skip optional commas and drop the final period (question marks stay).",
    "Hey are you free for lunch tomorrow? Let's do 12 if that works",
    "Dale nos vemos mañana tipo a las 8 en casa",
  ),
  very_casual: styleOverride(
    "very casual",
    "Lowercase everything except proper nouns — sentence starts included, even the first word. Keep apostrophes and question marks, skip commas, and never end with a period. Chat style.",
    "hey are you free for lunch tomorrow? let's do 12 if that works",
    "dale nos vemos mañana tipo a las 8 en casa",
  ),
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

// Mirrors prompt::apply_register: re-register base few-shot answers to the
// active tone so every example demonstrates the same register.
function applyRegister(text, tone) {
  if (tone === "formal") return text;
  let out = text
    .split("\n")
    .map((line) => line.replace(/(?<!\.)\.$/, ""))
    .join("\n");
  if (tone === "very_casual") {
    out = out.replace(/(^|[.!?]\s+|\n[-\s]*)(\p{L})/gmu, (_, pre, ch) => pre + ch.toLowerCase());
  }
  return out;
}

async function chat(system, transcript, styleShots = [], tone = "formal") {
  const started = performance.now();
  const fewShot = styleShots.length
    ? FEW_SHOT.map((m) =>
        m.role === "assistant" ? { ...m, content: applyRegister(m.content, tone) } : m,
      )
    : FEW_SHOT;
  const res = await fetch("http://localhost:11434/api/chat", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model,
      stream: false,
      options: { temperature: 0.1 },
      messages: [
        { role: "system", content: system },
        ...fewShot,
        // mirrors format.rs: the active tone demonstrates its register with
        // its own user/assistant turns (prompt::style_shots)
        ...styleShots.flatMap(([input, output]) => [
          { role: "user", content: input },
          { role: "assistant", content: output },
        ]),
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
      const { ms, text } = await chat(system, transcript, STYLE_SHOTS[tone], tone);
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
