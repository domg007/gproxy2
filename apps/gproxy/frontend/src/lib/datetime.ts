function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function isLeapYear(year: number): boolean {
  return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
}

function ordinalToMonthDay(year: number, ordinal: number): { month: number; day: number } | null {
  if (!Number.isInteger(ordinal) || ordinal <= 0 || ordinal > (isLeapYear(year) ? 366 : 365)) {
    return null;
  }

  const monthDays = [
    31,
    isLeapYear(year) ? 29 : 28,
    31,
    30,
    31,
    30,
    31,
    31,
    30,
    31,
    30,
    31
  ];

  let remain = ordinal;
  for (let month = 1; month <= monthDays.length; month += 1) {
    const days = monthDays[month - 1];
    if (remain <= days) {
      return { month, day: remain };
    }
    remain -= days;
  }
  return null;
}

function formatGmtOffset(date: Date): string {
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const abs = Math.abs(offsetMinutes);
  const hours = Math.floor(abs / 60);
  const minutes = abs % 60;
  if (minutes === 0) {
    return `${sign}${hours}`;
  }
  return `${sign}${hours}:${pad2(minutes)}`;
}

function fromOffsetDateTimeTuple(value: unknown): Date | null {
  if (!Array.isArray(value) || value.length < 9) {
    return null;
  }
  const items = value.map((item) => Number(item));
  if (items.some((item) => Number.isNaN(item))) {
    return null;
  }

  const [year, ordinal, hour, minute, second, nanosecond, offsetHours, offsetMinutes, offsetSeconds] =
    items;
  const md = ordinalToMonthDay(year, ordinal);
  if (!md) {
    return null;
  }

  const utcMs = Date.UTC(
    year,
    md.month - 1,
    md.day,
    hour,
    minute,
    second,
    Math.trunc(nanosecond / 1_000_000)
  );
  const offsetMs = (offsetHours * 3600 + offsetMinutes * 60 + offsetSeconds) * 1000;
  return new Date(utcMs - offsetMs);
}

function fromUnixLike(value: string | number): Date | null {
  const raw = typeof value === "number" ? value.toString() : value.trim();
  if (!raw || !/^-?\d+$/.test(raw)) {
    return null;
  }

  let int: bigint;
  try {
    int = BigInt(raw);
  } catch {
    return null;
  }

  const abs = int < 0n ? -int : int;
  let millis: bigint;
  if (abs >= 1_000_000_000_000_000_000n) {
    millis = int / 1_000_000n;
  } else if (abs >= 1_000_000_000_000_000n) {
    millis = int / 1_000n;
  } else if (abs >= 1_000_000_000_000n) {
    millis = int;
  } else {
    millis = int * 1_000n;
  }

  const asNumber = Number(millis);
  if (!Number.isFinite(asNumber)) {
    return null;
  }
  return new Date(asNumber);
}

function parseToDate(value: unknown): Date | null {
  const tupleDate = fromOffsetDateTimeTuple(value);
  if (tupleDate) {
    return tupleDate;
  }

  if (typeof value === "string") {
    const unixLike = fromUnixLike(value);
    if (unixLike) {
      return unixLike;
    }
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  if (typeof value === "number") {
    return fromUnixLike(value);
  }

  return null;
}

export function parseDateTimeLocalToUnixMs(value: string): number | null {
  const raw = value.trim();
  if (!raw) {
    return null;
  }

  // Preferred manual format: YYYY-MM-DD HH:mm (also accepts '/' and 'T', and HH-mm).
  const normalized = raw.replaceAll("/", "-");
  const manual = normalized.match(
    /^(\d{4})-(\d{1,2})-(\d{1,2})(?:[ T](\d{1,2})[:\-](\d{1,2}))?$/
  );
  if (manual) {
    const year = Number(manual[1]);
    const month = Number(manual[2]);
    const day = Number(manual[3]);
    const hour = Number(manual[4] ?? "0");
    const minute = Number(manual[5] ?? "0");
    if (
      Number.isNaN(year) ||
      Number.isNaN(month) ||
      Number.isNaN(day) ||
      Number.isNaN(hour) ||
      Number.isNaN(minute) ||
      month < 1 ||
      month > 12 ||
      day < 1 ||
      day > 31 ||
      hour < 0 ||
      hour > 23 ||
      minute < 0 ||
      minute > 59
    ) {
      return null;
    }

    const local = new Date(year, month - 1, day, hour, minute, 0, 0);
    if (
      local.getFullYear() !== year ||
      local.getMonth() !== month - 1 ||
      local.getDate() !== day ||
      local.getHours() !== hour ||
      local.getMinutes() !== minute
    ) {
      return null;
    }

    return local.getTime();
  }

  // Backward-compatible fallback for previously accepted values.
  const date = new Date(raw);
  return Number.isNaN(date.getTime()) ? null : date.getTime();
}

export function formatAtForViewer(value: unknown): string {
  const date = parseToDate(value);
  if (!date) {
    if (typeof value === "string") {
      return value;
    }
    if (Array.isArray(value)) {
      return value.join(",");
    }
    return String(value ?? "");
  }

  const year = date.getFullYear();
  const month = pad2(date.getMonth() + 1);
  const day = pad2(date.getDate());
  const hour = pad2(date.getHours());
  const minute = pad2(date.getMinutes());
  return `${year}-${month}-${day}:${hour}-${minute} GMT${formatGmtOffset(date)}`;
}

export function parseAtToUnixMs(value: unknown): number | null {
  const date = parseToDate(value);
  if (!date) {
    return null;
  }
  const ms = date.getTime();
  return Number.isFinite(ms) ? ms : null;
}
