# Code, with room to think

Use `Result<T, E>` to make errors explicit. Inline code stays in the sentence.

```rust
fn greet(name: &str) -> String {
    // Unicode belongs in code, too.
    let message = format!("Hello, {name} 🦀");

    message
}
```

```typescript
interface Theme {
  name: string;
  colors: Record<string, string>;
}

const theme: Theme = {
  name: "Midnight",
  colors: { accent: "#9eb5da" },
};
```

An empty block is ready for your next idea.

```python
```

Continue writing here.
