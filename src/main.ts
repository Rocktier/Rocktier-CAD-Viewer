import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// Sync persisted theme before first paint
const saved = localStorage.getItem("rcv.theme");
if (saved === "light" || saved === "dark") {
  document.documentElement.dataset.theme = saved;
}

const target = document.getElementById("app")!;
export default mount(App, { target });
