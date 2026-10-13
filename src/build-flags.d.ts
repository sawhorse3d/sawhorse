// Constants Vite writes into the build (vite.config.ts `define`).

/** Lane 17: page-context test hooks (window.__sawhorse.plugins) are compiled in: true in the
 * dev server, the unit tests and the end-to-end build; false (and the code dropped) in every
 * production build. */
declare const __SAWHORSE_TEST_HOOKS__: boolean;
