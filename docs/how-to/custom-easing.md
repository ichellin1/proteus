# Use a custom easing curve

An easing curve sets how a transition speeds up and slows down. Beyond the built-in curves, a
transition can use any cubic Bézier curve, and in Rust, any function.

## Copy a curve from CSS

A cubic Bézier curve takes the same four numbers as CSS's `cubic-bezier()`, so a curve from a
stylesheet, a design tool or an easing editor works as it is:

#### Rust

```rust
# use proteus_sdk::{Easing, TransitionConfig};
// CSS: transition-timing-function: cubic-bezier(0.2, 0, 0, 1);
let config = TransitionConfig {
    duration: 0.4,
    easing: Easing::CubicBezier {
        x1: 0.2,
        y1: 0.0,
        x2: 0.0,
        y2: 1.0,
    },
    ..Default::default()
};
```

#### TypeScript

```ts
// CSS: transition-timing-function: cubic-bezier(0.2, 0, 0, 1);
const config = { duration: 0.4, easing: { cubicBezier: [0.2, 0, 0, 1] } } as const;
```

Some curves to start from:

| Curve | Feels |
|---|---|
| `0.25, 0.1, 0.25, 1` | Gentle: CSS's `ease`. |
| `0.2, 0, 0, 1` | Starts quickly and settles slowly. Good for something arriving. |
| `0.34, 1.56, 0.64, 1` | Goes past its target and settles back. |
| `0.36, 0, 0.66, -0.56` | Pulls back before it goes. Good for something leaving. |

The second and fourth numbers can be below 0 or above 1, for a curve that goes past where it
starts or ends. The geometry follows it, within limits: a size or corner radius never goes below
zero, and a color stays within its range.

## Any function, in Rust

`Easing::Custom` takes a function from linear progress, `0` to `1`, to eased progress. It should
return `0` for `0` and `1` for `1`, or the transition jumps at its start or end. This one bounces
to a stop, like a dropped ball:

#### Rust

```rust
# use proteus_sdk::{Easing, TransitionConfig};
fn bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}
# assert!(bounce(0.0).abs() < 1e-6 && (bounce(1.0) - 1.0).abs() < 1e-6);

let config = TransitionConfig {
    duration: 0.8,
    easing: Easing::Custom(bounce),
    ..Default::default()
};
```

The function is a plain `fn`, not a closure, so it can't capture values. For a family of
curves, such as bounces of different heights, write a function for each.

TypeScript has the built-in curves and cubic Béziers only. A curve a cubic Bézier can't make,
such as a bounce, can be built from several transitions, one per segment; see
[Chain transitions](./chaining-transitions.md).
