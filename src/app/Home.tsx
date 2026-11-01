import { version as appVersion } from "../../package.json" with { type: "json" };

/**
 * Home — honest, no fake affordances. Tools register here (via the registry,
 * Milestone 3); until then the empty state is the design (PLAN §4.4).
 */
export function Home() {
  return (
    <div className="view-content">
      <section className="home-hero">
        <h1>OmniTools</h1>
        <p className="lede">
          Everyday file utilities that run entirely on your device.
        </p>
        <div className="facts">
          <span>Version {appVersion}</span>
          <span>Free &amp; open source</span>
          <span>No accounts</span>
          <span>Works offline</span>
        </div>
      </section>
    </div>
  );
}
