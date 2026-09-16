use std::{net::SocketAddr, time::Duration};

use castle_fight_server::{
    AuthoritativeMatch, DEFAULT_DISCONNECT_TIMEOUT, ServerMatchOptions, tcp::TcpAuthoritativeServer,
};
use castle_fight_sim::{
    CastleFightBuilderRace, CastleFightMatchConfig, CastleFightParticipantConfig, MapVersion,
    PlayerId, Team,
};

#[derive(Debug)]
struct ServerOptions {
    bind: SocketAddr,
    map_version: MapVersion,
    release_revision: String,
    seed: u64,
    team_size: usize,
    workers: usize,
    disconnect_timeout: Duration,
}

impl ServerOptions {
    fn parse() -> Self {
        let mut options = Self {
            bind: "127.0.0.1:6112"
                .parse()
                .expect("valid default bind address"),
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            seed: 0x4341_5354_4c45,
            team_size: 1,
            workers: default_worker_count(),
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bind" => {
                    options.bind = args
                        .next()
                        .expect("--bind requires an address such as 127.0.0.1:6112")
                        .parse()
                        .expect("--bind requires a valid socket address");
                }
                "--map-version" => {
                    options.map_version = args
                        .next()
                        .expect("--map-version requires a version such as 9.27")
                        .parse()
                        .expect("--map-version requires a registered map version");
                }
                "--map-revision" => {
                    options.release_revision = args
                        .next()
                        .expect("--map-revision requires an exact revision such as r1");
                }
                "--seed" => {
                    options.seed = args
                        .next()
                        .expect("--seed requires an unsigned integer")
                        .parse()
                        .expect("--seed requires an unsigned integer");
                }
                "--team-size" => {
                    options.team_size = args
                        .next()
                        .expect("--team-size requires 1, 2, or 3")
                        .parse()
                        .expect("--team-size requires 1, 2, or 3");
                    assert!(
                        (1..=3).contains(&options.team_size),
                        "--team-size requires 1, 2, or 3"
                    );
                }
                "--workers" => {
                    options.workers = args
                        .next()
                        .expect("--workers requires a positive integer")
                        .parse()
                        .expect("--workers requires a positive integer");
                    assert!(options.workers > 0, "--workers must be greater than zero");
                }
                "--disconnect-timeout-seconds" => {
                    let seconds = args
                        .next()
                        .expect("--disconnect-timeout-seconds requires an unsigned integer")
                        .parse()
                        .expect("--disconnect-timeout-seconds requires an unsigned integer");
                    options.disconnect_timeout = Duration::from_secs(seconds);
                }
                "-h" | "--help" => {
                    println!(
                        "Usage: castle-fight-server [--bind 127.0.0.1:6112] [--map-version 9.27] [--map-revision r1] [--seed N] [--team-size 1|2|3] [--workers N] [--disconnect-timeout-seconds N]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown server option: {unknown}"),
            }
        }
        options
    }

    fn match_config(&self) -> CastleFightMatchConfig {
        let western = [0u8, 1, 2];
        let eastern = [6u8, 7, 8];
        let participants =
            western
                .iter()
                .take(self.team_size)
                .map(|slot| CastleFightParticipantConfig {
                    id: PlayerId(*slot),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                })
                .chain(eastern.iter().take(self.team_size).map(|slot| {
                    CastleFightParticipantConfig {
                        id: PlayerId(*slot),
                        team: Team(1),
                        builder_race: CastleFightBuilderRace::Human,
                    }
                }))
                .collect();
        CastleFightMatchConfig::development_subset_with_participants(
            self.map_version,
            &self.release_revision,
            self.seed,
            participants,
        )
        .unwrap_or_else(|error| panic!("cannot create selected Castle Fight match: {error}"))
    }
}

fn main() {
    let options = ServerOptions::parse();
    let match_config = options.match_config();
    let authoritative = AuthoritativeMatch::new(
        match_config.clone(),
        options.workers,
        ServerMatchOptions {
            disconnect_timeout: options.disconnect_timeout,
            ..ServerMatchOptions::default()
        },
    )
    .unwrap_or_else(|error| panic!("cannot create authoritative match: {error}"));
    let server = TcpAuthoritativeServer::bind(options.bind, authoritative)
        .unwrap_or_else(|error| panic!("cannot bind server: {error}"));
    let local_addr = server
        .local_addr()
        .unwrap_or_else(|error| panic!("cannot inspect bound address: {error}"));
    println!(
        "Castle Fight server listening on {local_addr} — CF {}/{} — {}v{} — seed {}",
        match_config.release.map_version,
        match_config.release.release_revision,
        options.team_size,
        options.team_size,
        options.seed
    );
    println!(
        "Match simulation starts when the full initial roster has completed compatibility handshake."
    );
    server
        .run_until_match_end()
        .unwrap_or_else(|error| panic!("server terminated with error: {error}"));
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_team_size_uses_authored_slot_prefixes() {
        let options = ServerOptions {
            bind: "127.0.0.1:0".parse().unwrap(),
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            seed: 9,
            team_size: 2,
            workers: 1,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
        };
        let config = options.match_config();
        assert_eq!(
            config
                .participants
                .iter()
                .map(|participant| (participant.id.0, participant.team.0))
                .collect::<Vec<_>>(),
            vec![(0, 0), (1, 0), (6, 1), (7, 1)]
        );
    }
}
