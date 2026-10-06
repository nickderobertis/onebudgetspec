// Succeeds, explaining on stderr, but leaves a result that is not JSON.
const fs = require("node:fs");

process.stderr.write("could not reach the database; start it and re-run\n");
fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, "nope");
