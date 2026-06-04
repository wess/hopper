const SAFE = /^[a-zA-Z0-9._-]+$/;

const validate = (name: string, label: string): void => {
  if (!SAFE.test(name)) throw new Error(`${label} must match ${SAFE}; got: ${name}`);
};

const platform = (): "darwin" | "linux" | "win32" => {
  const p = process.platform;
  return p === "darwin" || p === "linux" || p === "win32" ? p : "linux";
};

const macSet = async (service: string, key: string, value: string): Promise<void> => {
  await Bun.$`security delete-generic-password -s ${service} -a ${key}`.quiet().nothrow();
  await Bun.$`security add-generic-password -s ${service} -a ${key} -w ${value}`.quiet();
};

const macGet = async (service: string, key: string): Promise<string | undefined> => {
  const proc = Bun.$`security find-generic-password -s ${service} -a ${key} -w`.quiet().nothrow();
  const result = await proc;
  if (result.exitCode !== 0) return undefined;
  return result.text().trim();
};

const macDelete = async (service: string, key: string): Promise<void> => {
  await Bun.$`security delete-generic-password -s ${service} -a ${key}`.quiet().nothrow();
};

const linuxSet = async (service: string, key: string, value: string): Promise<void> => {
  const proc = Bun.spawn(["secret-tool", "store", `--label=${key}`, "service", service, "key", key], {
    stdin: "pipe",
    stdout: "ignore",
    stderr: "ignore",
  });
  proc.stdin.write(value);
  await proc.stdin.end();
  await proc.exited;
};

const linuxGet = async (service: string, key: string): Promise<string | undefined> => {
  const r = await Bun.$`secret-tool lookup service ${service} key ${key}`.quiet().nothrow();
  if (r.exitCode !== 0) return undefined;
  return r.text().trim();
};

const linuxDelete = async (service: string, key: string): Promise<void> => {
  await Bun.$`secret-tool clear service ${service} key ${key}`.quiet().nothrow();
};

const winSet = async (service: string, key: string, value: string): Promise<void> => {
  const target = `${service}/${key}`;
  await Bun.$`cmdkey /generic:${target} /user:${key} /pass:${value}`.quiet();
};

const winGet = async (service: string, key: string): Promise<string | undefined> => {
  // cmdkey cannot retrieve the password value on stock Windows. Apps that
  // need cross-platform retrieval should ship a native helper or use a
  // dedicated keytar-style binary. We return undefined as a signal.
  void service;
  void key;
  return undefined;
};

const winDelete = async (service: string, key: string): Promise<void> => {
  await Bun.$`cmdkey /delete:${service}/${key}`.quiet().nothrow();
};

export const setSecret = async (service: string, key: string, value: string): Promise<void> => {
  validate(service, "service");
  validate(key, "key");
  switch (platform()) {
    case "darwin":
      return macSet(service, key, value);
    case "win32":
      return winSet(service, key, value);
    default:
      return linuxSet(service, key, value);
  }
};

export const getSecret = async (service: string, key: string): Promise<string | undefined> => {
  validate(service, "service");
  validate(key, "key");
  switch (platform()) {
    case "darwin":
      return macGet(service, key);
    case "win32":
      return winGet(service, key);
    default:
      return linuxGet(service, key);
  }
};

export const deleteSecret = async (service: string, key: string): Promise<void> => {
  validate(service, "service");
  validate(key, "key");
  switch (platform()) {
    case "darwin":
      return macDelete(service, key);
    case "win32":
      return winDelete(service, key);
    default:
      return linuxDelete(service, key);
  }
};
