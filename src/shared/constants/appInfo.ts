/** Canonical app facts — single source used by Settings, the About page, and docs. */
import { version as APP_VERSION } from "../../../package.json" with { type: "json" };

export const APP_NAME = "OmniTools";
export { APP_VERSION };

export const APP_LICENCE = "Apache-2.0";

export const PRIVACY_STATEMENT =
  "OmniTools performs no network requests and works without an account. Files are processed locally.";
