import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Overlay from "./Overlay";
import App from "./app/App";
import "./index.css";

const label = getCurrentWindow().label;

if (label === "overlay") {
  document.body.classList.add("overlay-window");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {label === "overlay" ? <Overlay /> : <App />}
  </React.StrictMode>,
);
