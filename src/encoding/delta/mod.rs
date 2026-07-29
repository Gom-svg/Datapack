pub fn encode_i64(values: &[i64]) -> Vec<i64> {
    let Some(first) = values.first() else {
        return Vec::new();
    };

    let mut output = Vec::with_capacity(values.len());
    output.push(*first);
    output.extend(values.windows(2).map(|pair| pair[1] - pair[0]));
    output
}
