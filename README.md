# tson

`tson` is a Rust CLI that reads TypeScript classes, interfaces, and type aliases and generates JSON Schema documents from their explicit type annotations.

It is intended for workflows where TypeScript classes describe the data shape you accept or emit, and you want a JSON Schema file that can be used by validators such as Ajv.

## Install

```sh
make install
```

By default this installs `tson` to `/usr/local/bin/tson`. To install somewhere else:

```sh
make install PREFIX="$HOME/.local"
```

Make sure the chosen `bin` directory is on your `PATH`.

## Quick Start

Create a TypeScript file:

```ts
// user.ts
export class User {
  id: string;
  name: string;
  age?: number;
  tags: string[];
  address: {
    street: string;
    city: string;
    zip?: number;
  };
}
```

Generate a schema:

```sh
tson generate user.ts --class User --out user.schema.json
```

You can also use the short default form:

```sh
tson user.ts -c User -o user.schema.json
```

The generated schema will look like:

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "name": { "type": "string" },
    "age": { "type": "number" },
    "tags": {
      "type": "array",
      "items": { "type": "string" }
    },
    "address": {
      "type": "object",
      "properties": {
        "street": { "type": "string" },
        "city": { "type": "string" },
        "zip": { "type": "number" }
      },
      "required": ["street", "city"],
      "additionalProperties": false
    }
  },
  "required": ["id", "name", "tags", "address"],
  "additionalProperties": false,
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "User"
}
```

## Referenced Types

`tson` can resolve classes, interfaces, and type aliases declared in the same input set. Referenced types are emitted under `$defs`.

```ts
export type Role = "admin" | "user";

export interface Profile {
  role: Role;
  active: boolean;
}

export class User {
  id: string;
  profile: Profile;
}
```

```sh
tson generate user.ts --class User
```

The `profile` property will become:

```json
{ "$ref": "#/$defs/Profile" }
```

and `Profile` plus `Role` will be emitted in `$defs`.

## Multiple Files

Pass multiple files when a root class references types declared elsewhere:

```sh
tson generate src/user.ts src/profile.ts src/role.ts --class User --out user.schema.json
```

`tson` parses the files together and resolves type references across the provided files.

## Supported TypeScript Shapes

`tson` currently supports:

- classes with typed public fields
- interfaces with property signatures
- type aliases
- nested object literal types
- optional properties using `?`
- `string`, `number`, `boolean`, `bigint`, `object`, `null`, `any`, and `unknown`
- arrays using `T[]`
- `Array<T>` and `ReadonlyArray<T>`
- unions, including nullable unions and string/number/boolean literal enums
- references to local classes, interfaces, and type aliases

Unsupported type expressions are kept permissive and include a `description` explaining what could not be converted.

## CLI

```sh
tson generate <FILE>... [--class NAME] [--out FILE] [--allow-additional]
```

Options:

- `-c, --class NAME`: root class or interface to generate. If omitted, `tson` uses the first class or interface found.
- `-o, --out FILE`: write the schema to a file. If omitted, schema JSON is printed to stdout.
- `--allow-additional`: set `additionalProperties` to `true` on generated objects.

The default invocation is equivalent to `generate`:

```sh
tson <FILE>... -c User -o user.schema.json
```

## Development

```sh
make test
make build
```

The CLI is implemented in Rust and uses `tree-sitter-typescript` to parse TypeScript source.
