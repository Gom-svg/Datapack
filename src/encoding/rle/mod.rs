#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<T> {
    pub value: T,
    pub count: usize,
}

pub fn encode<T: Clone + Eq>(values: &[T]) -> Vec<Run<T>> {
    let mut runs = Vec::new();
    let Some(first) = values.first() else {
        return runs;
    };

    let mut current = first.clone();
    let mut count = 1;
    for value in &values[1..] {
        if value == &current {
            count += 1;
        } else {
            runs.push(Run {
                value: current,
                count,
            });
            current = value.clone();
            count = 1;
        }
    }
    runs.push(Run {
        value: current,
        count,
    });
    runs
}
