import React from "react";
import ReactDOM from "react-dom/client";
import "@xyflow/react/dist/style.css";
import "./styles.css";
import { App } from "./ui/App";
import { isMac } from "./platform";

// Platform-specific style tokens key off this attribute in styles.css.
if (isMac) document.documentElement.setAttribute("data-platform", "mac");

const root = document.getElementById("root");
if (root === null) {
  throw new Error("root element is missing");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
