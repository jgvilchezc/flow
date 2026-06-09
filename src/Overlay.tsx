import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { OverlayState } from "./types";

export default function Overlay() {
  const [overlay, setOverlay] = useState<OverlayState>({
    state: "idle",
    message: "",
  });

  useEffect(() => {
    const unlisten = listen<OverlayState>("flow://state", (event) => {
      setOverlay(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className={`pill pill--${overlay.state}`}>
      {overlay.state === "recording" && (
        <>
          <span className="pill__dot" />
          <span className="pill__bars">
            <i />
            <i />
            <i />
            <i />
            <i />
          </span>
          <span className="pill__label">Listening</span>
        </>
      )}
      {overlay.state === "processing" && (
        <>
          <span className="pill__spinner" />
          <span className="pill__label">Transcribing…</span>
        </>
      )}
      {overlay.state === "error" && (
        <span className="pill__label pill__label--error">
          {overlay.message || "Something went wrong"}
        </span>
      )}
    </div>
  );
}
