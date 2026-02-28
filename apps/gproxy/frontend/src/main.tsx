import { StrictMode } from "react";
import ReactDOM from "react-dom/client";

import { App } from "./app/App";
import { I18nProvider } from "./app/i18n";
import "./styles/index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>
);
