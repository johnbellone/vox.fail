// The front page's hero: the Void as it is right now, watched from a camera drifting round
// where people arrive. The still Void backdrop shows first and stays when the live view can't
// run (no WebGPU, reduced motion, a phone, data saver) or the world can't be reached.

import { useEffect, useState } from "react";
import { api, worldUrl } from "../api/client";
import type { Worlds } from "../api/types";
import { canShowLive, loadClient } from "../game/wasm";

export function LiveWorld() {
  const [live, setLive] = useState(false);

  useEffect(() => {
    let alive = true;
    window.vox_ready = () => alive && setLive(true);
    window.vox_status = () => {};
    window.vox_error = (text) => console.warn("live view:", text);
    (async () => {
      if (!(await canShowLive()) || !alive) return;
      const w = await api<Worlds>("/api/worlds");
      const world = w.worlds?.[0];
      if (!world || !alive) return;
      const start = await loadClient();
      if (!alive) return;
      start({ canvas: "#live", url: worldUrl(world.url), watch: true });
    })().catch((err) => console.warn("no live view:", err));
    return () => {
      // the client can't be stopped once running; it ends with the page
      alive = false;
    };
  }, []);

  return (
    <div className={`vox-live${live ? " on" : ""}`} aria-hidden="true">
      <canvas id="live" tabIndex={-1} />
      <div className="veil" />
    </div>
  );
}
