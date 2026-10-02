use vector::{Config, Result, VectorIndex};

fn main() -> Result<()> {
    let mut index = VectorIndex::new(Config::new(3))?;
    index.put(1, &[1.0, 0.0, 0.0], "첫 번째 벡터")?;
    index.put(2, &[0.0, 1.0, 0.0], "두 번째 벡터")?;
    index.put(3, &[0.0, 0.0, 1.0], "세 번째 벡터")?;
    let result = index.search_hnsw(&[1.0, 0.1, 0.0], 2, 64)?;
    for neighbor in result.neighbors {
        println!(
            "id={} squared_l2={:.6} metadata={}",
            neighbor.id, neighbor.distance, neighbor.metadata
        );
    }
    index.check_invariants()?;
    Ok(())
}
