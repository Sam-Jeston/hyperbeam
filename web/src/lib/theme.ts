// Dark by default. The choice is per browser; index.html applies it before first paint.

import { useSyncExternalStore } from "react";

export type ThemeChoice = "dark" | "light" | "system";

const KEY = "noemune:theme";
const listeners = new Set<() => void>();
const media = window.matchMedia("(prefers-color-scheme: light)");

function read(): ThemeChoice {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "system" ? v : "dark";
  } catch {
    return "dark";
  }
}

function apply(choice: ThemeChoice) {
  const light = choice === "light" || (choice === "system" && media.matches);
  document.documentElement.className = light ? "light" : "dark";
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", light ? "#f2f4f1" : "#0f1411");
}

media.addEventListener("change", () => read() === "system" && apply("system"));

export function setTheme(choice: ThemeChoice) {
  try {
    localStorage.setItem(KEY, choice);
  } catch {
    // Private windows may refuse; the choice still applies until reload.
  }
  apply(choice);
  listeners.forEach((fn) => fn());
}

export function useTheme(): ThemeChoice {
  return useSyncExternalStore((fn) => (listeners.add(fn), () => listeners.delete(fn)), read);
}
