use tokio::{pin, select, spawn, sync::mpsc, time};

use model::*;

use super::*;

mod search_tree;

use search_tree::*;

const NO_TIME: Duration = Duration::from_secs(0);

pub async fn respond_move(
    game: &Game,
    timeout: Duration,
    model: Arc<Model>,
) -> Result<MoveResponse> {
    let direction = make_move(game, timeout, model).await?;
    info!(?direction);
    Ok(MoveResponse::new(direction))
}

#[instrument(skip(game, model))]
async fn make_move(game: &Game, timeout: Duration, model: Arc<Model>) -> Result<Direction> {
    info!(?game);

    let (sender, mut receiver) = mpsc::channel(8);

    let searches = tree_searches(game.clone(), sender, model);
    let searches = time::timeout(timeout, searches);
    let searches = async move {
        match searches.await {
            Ok(Err(why)) => error!(?why, "tree_searches"),
            Ok(Ok(())) | Err(_) => {}
        }
    };
    let mut searches = spawn(searches);

    let extra_safety_sleeper = time::sleep(timeout);
    pin!(extra_safety_sleeper);

    let mut direction = None;
    while let Some(new_direction) = select! {
        new_direction = receiver.recv() => new_direction,
        _ = &mut searches => None,
        _ = &mut extra_safety_sleeper => None,
    } {
        direction = Some(new_direction);
        debug!(?new_direction);
    }

    let direction = direction.context("Tree search did not return a direction")?;
    info!(?direction);
    Ok(direction)
}

/// To avoid out of RAM.
const TOO_MANY_NODES: usize = 0x8_000;

async fn tree_searches(
    game: Game,
    sender: mpsc::Sender<Direction>,
    model: Arc<Model>,
) -> Result<()> {
    let mut search_tree = SearchTree::try_new(game, &model).await?;

    loop {
        if search_tree.nodes.len() >= TOO_MANY_NODES {
            warn!(
                n_search_tree_node = search_tree.nodes.len(),
                "Too many nodes"
            );
            break;
        }
        let more_layers_exist = search_tree.compute_next_layer(&model).await?;
        let new_direction = search_tree
            .best_direction()
            .context("No result from search tree")?;
        sender.send(new_direction).await?;
        debug!(?new_direction, ?search_tree.depth, "Sent");
        if !more_layers_exist {
            info!("Search complete");
            break;
        }
    }

    Ok(())
}

fn actions_with_replaced(
    actions: [Direction; 4],
    replace_index: usize,
    direction: Direction,
) -> [Direction; 4] {
    let mut new_actions = actions;
    new_actions[replace_index] = direction;
    new_actions
}
