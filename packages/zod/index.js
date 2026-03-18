class StringSchema {
  constructor(options = {}) {
    this.minLength = options.minLength ?? null;
    this.maxLength = options.maxLength ?? null;
    this.pattern = options.pattern ?? null;
  }

  min(minLength) {
    return new StringSchema({ ...this, minLength });
  }

  max(maxLength) {
    return new StringSchema({ ...this, maxLength });
  }

  regex(pattern) {
    return new StringSchema({ ...this, pattern });
  }

  parse(value) {
    if (typeof value !== "string") {
      throw new TypeError(`Expected string, received ${typeof value}`);
    }
    if (this.minLength !== null && value.length < this.minLength) {
      throw new TypeError(`Expected string length >= ${this.minLength}`);
    }
    if (this.maxLength !== null && value.length > this.maxLength) {
      throw new TypeError(`Expected string length <= ${this.maxLength}`);
    }
    if (this.pattern && !this.pattern.test(value)) {
      throw new TypeError(`Expected string to match ${this.pattern}`);
    }
    return value;
  }
}

class NumberSchema {
  constructor(options = {}) {
    this.integer = options.integer ?? false;
    this.minimum = options.minimum ?? null;
    this.maximum = options.maximum ?? null;
  }

  int() {
    return new NumberSchema({ ...this, integer: true });
  }

  min(minimum) {
    return new NumberSchema({ ...this, minimum });
  }

  max(maximum) {
    return new NumberSchema({ ...this, maximum });
  }

  parse(value) {
    if (typeof value !== "number" || Number.isNaN(value)) {
      throw new TypeError(`Expected number, received ${typeof value}`);
    }
    if (this.integer && !Number.isInteger(value)) {
      throw new TypeError("Expected integer");
    }
    if (this.minimum !== null && value < this.minimum) {
      throw new TypeError(`Expected number >= ${this.minimum}`);
    }
    if (this.maximum !== null && value > this.maximum) {
      throw new TypeError(`Expected number <= ${this.maximum}`);
    }
    return value;
  }
}

class BooleanSchema {
  parse(value) {
    if (typeof value !== "boolean") {
      throw new TypeError(`Expected boolean, received ${typeof value}`);
    }
    return value;
  }
}

class ArraySchema {
  constructor(itemSchema, options = {}) {
    this.itemSchema = itemSchema;
    this.minimum = options.minimum ?? null;
    this.maximum = options.maximum ?? null;
  }

  min(minimum) {
    return new ArraySchema(this.itemSchema, { ...this, minimum });
  }

  max(maximum) {
    return new ArraySchema(this.itemSchema, { ...this, maximum });
  }

  parse(value) {
    if (!Array.isArray(value)) {
      throw new TypeError(`Expected array, received ${typeof value}`);
    }
    if (this.minimum !== null && value.length < this.minimum) {
      throw new TypeError(`Expected array length >= ${this.minimum}`);
    }
    if (this.maximum !== null && value.length > this.maximum) {
      throw new TypeError(`Expected array length <= ${this.maximum}`);
    }
    return value.map((item) => this.itemSchema.parse(item));
  }
}

class ObjectSchema {
  constructor(shape, options = {}) {
    this.shape = shape;
    this.strictMode = options.strictMode ?? false;
  }

  strict() {
    return new ObjectSchema(this.shape, { ...this, strictMode: true });
  }

  parse(value) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      throw new TypeError(`Expected object, received ${typeof value}`);
    }

    const parsed = {};
    for (const [key, schema] of Object.entries(this.shape)) {
      parsed[key] = schema.parse(value[key]);
    }

    if (this.strictMode) {
      for (const key of Object.keys(value)) {
        if (!(key in this.shape)) {
          throw new TypeError(`Unexpected key ${key}`);
        }
      }
    }

    return parsed;
  }
}

export const z = {
  string() {
    return new StringSchema();
  },
  number() {
    return new NumberSchema();
  },
  boolean() {
    return new BooleanSchema();
  },
  array(itemSchema) {
    return new ArraySchema(itemSchema);
  },
  object(shape) {
    return new ObjectSchema(shape);
  }
};
