// Fails, writing a byte that is not UTF-8 (0xFF) into its stderr.
process.stderr.write(
  Buffer.concat([
    Buffer.from("sensor returned "),
    Buffer.from([0xff]),
    Buffer.from("; reconnect the sensor and re-run\n"),
  ]),
);
process.exitCode = 2;
