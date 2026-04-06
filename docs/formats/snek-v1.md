# .snek Preset Format (Version 1)

SnenkBridge preset files use the `.snek` extension. The format is JSON with a metadata envelope wrapping the parameter configuration.

## Schema

```json
{
  "format": "snek",
  "version": 1,
  "title": "Preset Name",
  "author": "Author Name",
  "description": "What this preset does",
  "params": [ ... ]
}
```

### Top-level fields

- `format` (string, required): Always "snek".
- `version` (integer, required): Schema version. Currently 1.
- `title` (string, required): Display name for the preset.
- `author` (string, optional): Preset author. Omitted from output when empty.
- `description` (string, optional): Freeform description. Omitted from output when empty.
- `params` (array, required): Array of parameter objects. See [json-config.md](json-config.md) for the parameter format.

### Versioning

The app loads presets based on the `version` field. Older versions are always supported. If the app encounters a version it does not understand, it shows an error rather than loading incorrect data.

### Import compatibility

The app also accepts bare JSON arrays of parameter objects (the pre-.snek format). These are loaded with empty metadata. The `.snek` envelope is detected by checking whether the top-level JSON value is an object with a `"format": "snek"` field.
