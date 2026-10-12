// What an import or open did, in plain words: what became solids, what stayed a mesh and
// why, what was healed or skipped. Shown over the viewport until dismissed.

export interface Report {
  title: string;
  lines: string[];
  error?: boolean;
}

export function ImportReport({ report, onClose }: { report: Report; onClose: () => void }) {
  return (
    <section className={`import-report${report.error ? " err" : ""}`} role="status" aria-label="Import report" data-testid="import-report">
      <header>
        <b>{report.title}</b>
        <button type="button" aria-label="Close the import report" onClick={onClose}>
          ×
        </button>
      </header>
      {report.lines.length ? (
        <ul>
          {report.lines.map((l, i) => (
            <li key={i}>{l}</li>
          ))}
        </ul>
      ) : (
        <p>Nothing was healed, skipped or left as a mesh.</p>
      )}
    </section>
  );
}
