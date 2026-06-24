import React from "react";
import ReactDOM from "react-dom/client";

import App from "@/app/App";
import { Providers } from "@/app/providers";
import { AppErrorBoundary } from "@/components/shell/AppErrorBoundary";
import { createLogger, installGlobalErrorLogging } from "@/lib/logger";
import "@/i18n";
import "@/styles/globals.css";

installGlobalErrorLogging(createLogger("global"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Providers>
      <AppErrorBoundary>
        <App />
      </AppErrorBoundary>
    </Providers>
  </React.StrictMode>,
);
