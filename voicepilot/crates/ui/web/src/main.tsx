import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { PetWindow } from "./components/PetWindow";
import "./styles.css";

// 桌宠化改造:同一 bundle 按 Tauri 窗口 label 分流根组件。
// main → 完整控制台;pet → 谛听桌宠。浏览器预览(无 Tauri)按 main 处理。
function isPetWindow(): boolean {
  try {
    return getCurrentWindow().label === "pet";
  } catch {
    return false;
  }
}

// pet 窗口需要透明底(styles.css 的 html.pet-mode 规则);主窗口保持正常底色
const pet = isPetWindow();
if (pet) {
  document.documentElement.classList.add("pet-mode");
}

// 诊断探针(B8:main 窗口此前无任何 JS 错误上报,现两窗口统一注册):
// - 必须在任何可能抛错的表达之前注册 error/rejection 监听;
// - invoke 走静态导入,避免动态 import 的微任务时序把早期探针吞掉;
// - 复用 pet_probe 的 eprintln 通道,消息前缀区分窗口,零后端改动。
const probe = (msg: string): void => {
  try {
    invoke("pet_probe", { status: msg }).catch(() => {});
  } catch {
    /* ignore */
  }
};
window.addEventListener("error", (e) => {
  probe(`${pet ? "" : "main "}ERR ${String(e.message).slice(0, 160)}`);
});
window.addEventListener("unhandledrejection", (e) => {
  probe(
    `${pet ? "" : "main "}REJ ${String((e as PromiseRejectionEvent).reason).slice(0, 160)}`,
  );
});
if (pet) {
  // webgl2 检测:IIFE 单次赋值,避免 try/catch 下的 useless-assignment
  const webgl2 = ((): boolean => {
    try {
      return !!document.createElement("canvas").getContext("webgl2");
    } catch {
      return false;
    }
  })();
  probe(`js-alive webgl2=${webgl2} label=${getCurrentWindow().label}`);
} else {
  probe(`main js-alive label=${getCurrentWindow().label}`);
}
(window as unknown as { __probe: (m: string) => void }).__probe = probe;

const Root = pet ? PetWindow : App;

try {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Root />
    </React.StrictMode>,
  );
  (window as unknown as { __probe?: (m: string) => void }).__probe?.(
    "render-scheduled",
  );
} catch (e) {
  (window as unknown as { __probe?: (m: string) => void }).__probe?.(
    `RENDER-THREW ${e instanceof Error ? e.stack?.slice(0, 200) : String(e)}`,
  );
}
