import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Overlay from "./Overlay";
import Settings from "./Settings";
import "./index.css";

const label = getCurrentWindow().label;

if (label === "overlay") {
  document.body.classList.add("overlay-window");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {label === "overlay" ? <Overlay /> : <Settings />}
  </React.StrictMode>,
);
