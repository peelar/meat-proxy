use crate::model::PullRequest;
use std::collections::{HashMap, HashSet};

pub struct StackRow<'a> {
    pub pr: &'a PullRequest,
    pub depth: usize,
    pub parent: Option<u64>,
}

/// Input is one repository's visible PRs in inbox priority order.
pub fn rows<'a>(visible: &'a [PullRequest], inbox: &[PullRequest]) -> Vec<StackRow<'a>> {
    let Some(first) = visible.first() else {
        return Vec::new();
    };
    let mut heads = HashMap::new();
    for pr in inbox.iter().filter(|pr| pr.repo == first.repo) {
        if pr
            .head_repo
            .as_deref()
            .is_some_and(|repo| repo.eq_ignore_ascii_case(&first.repo))
            && !pr.head_ref.is_empty()
        {
            // A branch with two open PRs has no unique parent, even if one is filtered out.
            heads
                .entry(pr.head_ref.as_str())
                .and_modify(|id| *id = None)
                .or_insert(Some(pr.number));
        }
    }
    let positions: HashMap<_, _> = visible
        .iter()
        .enumerate()
        .map(|(i, pr)| (pr.number, i))
        .collect();
    let parents: Vec<_> = visible
        .iter()
        .map(|pr| {
            let number = heads.get(pr.stack_base.as_deref()?)?.as_ref()?;
            positions
                .get(number)
                .copied()
                .filter(|_| *number != pr.number)
        })
        .collect();
    let mut children = vec![Vec::new(); visible.len()];
    for (i, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(i);
        }
    }

    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(visible.len());
    for i in 0..visible.len() {
        if seen.contains(&i) {
            continue;
        }
        let mut root = i;
        let mut ancestors = HashSet::new();
        while let Some(parent) = parents[root] {
            if !ancestors.insert(root) {
                break;
            }
            root = parent;
        }
        if parents[root].is_some() {
            // Mutually targeted branches are not a stack.
            seen.insert(i);
            result.push(StackRow {
                pr: &visible[i],
                depth: 0,
                parent: None,
            });
            continue;
        }
        let mut pending = vec![(root, 0)];
        while let Some((index, depth)) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            result.push(StackRow {
                pr: &visible[index],
                depth,
                parent: parents[index].map(|p| visible[p].number),
            });
            pending.extend(
                children[index]
                    .iter()
                    .rev()
                    .map(|child| (*child, depth + 1)),
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgent_child_brings_its_stack_forward_and_hidden_parent_stays_hidden() {
        let all = crate::scenes::demo_prs();
        let by_number = |id| all.iter().find(|p| p.number == id).unwrap().clone();
        let visible = vec![
            by_number(421),
            by_number(397),
            by_number(418),
            by_number(412),
        ];
        let layout = |prs: &[PullRequest]| {
            rows(prs, &all)
                .iter()
                .map(|r| (r.pr.number, r.depth))
                .collect::<Vec<_>>()
        };
        assert_eq!(layout(&visible), [(418, 0), (412, 1), (421, 2), (397, 0)]);
        assert_eq!(layout(&visible[..3]), [(421, 0), (397, 0), (418, 0)]);
    }

    #[test]
    fn fork_heads_ambiguous_branches_and_cycles_do_not_create_false_stacks() {
        let all = crate::scenes::demo_prs();
        let mut prs: Vec<_> = all
            .into_iter()
            .filter(|p| matches!(p.number, 412 | 418))
            .collect();
        prs[0].head_repo = Some("fork/rune".into());
        assert!(rows(&prs, &prs).iter().all(|r| r.depth == 0));
        prs[0].head_repo = Some(prs[0].repo.clone());
        let mut ambiguous = prs.clone();
        let mut duplicate = prs[0].clone();
        duplicate.number = 999;
        ambiguous.push(duplicate);
        assert!(rows(&prs, &ambiguous).iter().all(|r| r.depth == 0));
        prs[0].stack_base = Some(prs[1].head_ref.clone());
        assert_eq!(
            rows(&prs, &prs).iter().map(|r| r.depth).collect::<Vec<_>>(),
            [0, 0]
        );
    }
}
