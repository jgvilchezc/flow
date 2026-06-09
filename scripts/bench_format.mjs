// Benchmarks Flow's formatting pass: same endpoint, body and params as
// src-tauri/src/format.rs, with the SYSTEM_PROMPT extracted from the source.
import { readFileSync } from "node:fs";

const model = process.argv[2] ?? "qwen2.5:7b";
const source = readFileSync(
  new URL("../src-tauri/src/format.rs", import.meta.url),
  "utf8",
);
const match = source.match(/const SYSTEM_PROMPT: &str = r#"([\s\S]*?)"#;/);
if (!match) throw new Error("SYSTEM_PROMPT not found in format.rs");
const SYSTEM_PROMPT = match[1];

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
