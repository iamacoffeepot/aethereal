// Oklab to linear sRGB: the colour `lab` holds as lightness, then a and b, in the linear sRGB it names.
public fn from_oklab(lab: vec3<f32>) -> vec3<f32> {
    let l0 = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m0 = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s0 = lab.x - 0.0894841775 * lab.y - 1.291485548 * lab.z;
    let l = l0 * l0 * l0;
    let m = m0 * m0 * m0;
    let s = s0 * s0 * s0;
    return vec3(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    );
}

// Linear sRGB encoded as sRGB, each channel: linear below 0.0031308, the 1 / 2.4 power above it.
public fn encode(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055, 12.92 * linear, linear <= vec3(0.0031308));
}

// The filmic tone curve, each channel: `x(2.51x + 0.03) / (x(2.43x + 0.59) + 0.14)`, clamped to [0, 1]. It takes
// linear light, of any brightness, to linear light a display shows; `encode` follows it.
public fn tone(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3(0.0), vec3(1.0));
}
