import { APP_LICENCE, APP_VERSION, PRIVACY_STATEMENT } from "../../../shared/constants/appInfo";

/** Registered under the tool registry (id "about") — a real info page, not a placeholder. */
export default function AboutPage() {
  return (
    <div className="view-content">
      <div className="settings-sections">
        <section className="settings-section">
          <h1 className="page-title">About</h1>
          <dl className="kv">
            <dt>Version</dt>
            <dd>{APP_VERSION}</dd>
            <dt>Licence</dt>
            <dd>{APP_LICENCE}</dd>
            <dt>Privacy</dt>
            <dd>{PRIVACY_STATEMENT}</dd>
            <dt>Data</dt>
            <dd>OmniTools reads and writes files only where you ask it to. Nothing is uploaded.</dd>
          </dl>
        </section>
      </div>
    </div>
  );
}
