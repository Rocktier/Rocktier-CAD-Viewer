import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// Sync persisted theme before first paint.  localStorage throws outright when
// site data is blocked, which would otherwise blank the whole app.
let saved: string | null = null;
try {
  saved = localStorage.getItem("rcv.theme");
} catch {
  saved = null;
}
if (saved === "light" || saved === "dark") {
  document.documentElement.dataset.theme = saved;
}

const target = document.getElementById("app")!;
export default mount(App, { target });
