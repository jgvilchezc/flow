// Benchmarks Flow's formatting pass: same endpoint, body and params as
// src-tauri/src/format.rs, sharing the prompt files under src-tauri/prompts/.
import { readFileSync } from "node:fs";

const model = process.argv[2] ?? "qwen2.5:7b";
const SYSTEM_PROMPT = readFileSync(
  new URL("../src-tauri/prompts/system_prompt.txt", import.meta.url),
  "utf8",
);
const FEW_SHOT = JSON.parse(
  readFileSync(new URL("../src-tauri/prompts/few_shot.json", import.meta.url), "utf8"),
);

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
  const started = performance.now();
  const res = await fetch("http://localhost:11434/api/chat", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model,
      stream: false,
      options: { temperature: 0.1 },
      messages: [
        { role: "system", content: SYSTEM_PROMPT },
        ...FEW_SHOT,
        { role: "user", content: transcript },
      ],
    }),
  });
  const body = await res.json();
  const ms = Math.round(performance.now() - started);
  const text = (body.message?.content ?? "<ERROR>").trim();
  console.log(`── ${name} [${ms} ms]`);
  console.log(`   in : ${transcript}`);
  console.log(`   out: ${text.replaceAll("\n", "\n        ")}`);
  console.log();
}
