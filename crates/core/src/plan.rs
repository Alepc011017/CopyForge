use std::path::PathBuf;

use crate::options::CopyOptions;

#[derive(Debug, Clone)]
pub struct CopyPlan {
    pub tasks: Vec<CopyTask>,
    pub options: CopyOptions,
    pub total_bytes: u64,
}

impl CopyPlan {
    /// Añade las tareas de `other` al final y suma sus bytes.
    ///
    /// **No fusiona `options`**: `CopyEngine::run` usa `self.options` (las del
    /// engine), no las del plan, así que este helper solo concatena tareas y
    /// bytes. Si `other.options` difiere de `self.options`, el comportamiento es
    /// el de `self` (documentado en `PLAN.md` P4.6a).
    pub fn extend(&mut self, other: CopyPlan) {
        self.total_bytes += other.total_bytes;
        self.tasks.extend(other.tasks);
    }
}

#[derive(Debug, Clone)]
pub struct CopyTask {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub size: u64,
    pub kind: TaskKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind { File, Dir, Symlink }

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str, size: u64) -> CopyTask {
        CopyTask {
            src: PathBuf::from(format!("/src/{name}")),
            dst: PathBuf::from(format!("/dst/{name}")),
            size,
            kind: TaskKind::File,
        }
    }

    fn plan(sizes: &[u64]) -> CopyPlan {
        CopyPlan {
            tasks: sizes
                .iter()
                .enumerate()
                .map(|(i, size)| task(&format!("f{i}"), *size))
                .collect(),
            options: CopyOptions::default(),
            total_bytes: sizes.iter().sum(),
        }
    }

    #[test]
    fn extend_concatenates_tasks_and_sums_bytes() {
        let mut a = plan(&[10, 20, 30]);
        let b = plan(&[5, 7]);

        a.extend(b);

        assert_eq!(a.tasks.len(), 5);
        assert_eq!(a.total_bytes, 72);
        // El orden se conserva: primero las de `a`, luego las de `b`.
        assert_eq!(a.tasks[0].src, PathBuf::from("/src/f0"));
        assert_eq!(a.tasks[3].src, PathBuf::from("/src/f0"));
        assert_eq!(a.tasks[4].src, PathBuf::from("/src/f1"));
    }
}
