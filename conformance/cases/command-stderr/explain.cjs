// Fails, explaining why across a blank line and an indented, padded line.
process.stderr.write("telemetry is absent\n\n   run the collector first  \n");
process.exitCode = 1;
