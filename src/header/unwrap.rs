use crate::constants::{INTEGRITY_MAC_SIZE, SECURITY_PARAMETER, STREAM_CIPHER_OUTPUT_LENGTH};
use crate::header::header;
use crate::header::header::MixNode;
use crate::header::routing;
use crate::header::routing::{
    PaddedRoutingInformation, RoutingInformation, RoutingKeys, StreamCipherKey, ROUTING_INFO_SIZE,
};
use crate::header::SphinxHeader;
use crate::utils;
use crate::utils::crypto;
use crate::Hop;

pub fn unwrap_routing_information(
    header: SphinxHeader,
    stream_cipher_key: &StreamCipherKey,
) -> (SphinxHeader, Hop) {
    // we have to add padding to the encrypted routing information before decrypting, otherwise we gonna lose informatio
    let padded_routing_information =
        add_zero_padding_to_encrypted_routing_information(&header.routing_info.enc_header);
    let unwrapped_routing_info =
        decrypt_padded_routing_info(stream_cipher_key, &padded_routing_information);

    // TODO: parse the decrypted result to get next_hop, delay, next_routing_info etc.

    (
        SphinxHeader {
            shared_secret: curve25519_dalek::montgomery::MontgomeryPoint([0u8; 32]),
            routing_info: routing::RoutingInfo {
                enc_header: [0u8; ROUTING_INFO_SIZE],
                header_integrity_hmac: [0u8; INTEGRITY_MAC_SIZE],
            },
        },
        Hop {
            host: header::RouteElement::ForwardHop(MixNode {
                address: header::node_address_fixture(),
                pub_key: curve25519_dalek::montgomery::MontgomeryPoint([0u8; 32]),
            }),
            delay: 0.0,
        },
    )
}

fn add_zero_padding_to_encrypted_routing_information(enc_routing_info: &[u8]) -> Vec<u8> {
    let zero_bytes = vec![0u8; 3 * SECURITY_PARAMETER];
    [enc_routing_info.to_vec(), zero_bytes.to_vec()].concat()
}

pub fn check_integrity_mac(
    integrity_mac: routing::HeaderIntegrityMac,
    integrity_mac_key: routing::HeaderIntegrityMacKey,
    enc_routing_info: RoutingInformation,
) -> bool {
    let recomputed_integrity_mac =
        routing::generate_routing_info_integrity_mac(integrity_mac_key, enc_routing_info);
    if integrity_mac != recomputed_integrity_mac {
        return false;
    }
    return true;
}

pub fn decrypt_padded_routing_info(
    key: &StreamCipherKey,
    padded_routing_info: &[u8],
) -> PaddedRoutingInformation {
    let pseudorandom_bytes = crypto::generate_pseudorandom_bytes(
        &key,
        &crypto::STREAM_CIPHER_INIT_VECTOR,
        STREAM_CIPHER_OUTPUT_LENGTH,
    );

    let lenx = padded_routing_info.len();
    let decrypted_routing_info_vec = utils::bytes::xor(&padded_routing_info, &pseudorandom_bytes);

    let mut decrypted_routing_info = [0u8; ROUTING_INFO_SIZE + 3 * SECURITY_PARAMETER];
    decrypted_routing_info.copy_from_slice(&decrypted_routing_info_vec);
    decrypted_routing_info
}

#[cfg(test)]
mod checking_integrity_mac {
    use super::*;
    use crate::constants::INTEGRITY_MAC_KEY_SIZE;

    #[test]
    fn it_returns_true_if_mac_matching() {
        let data = [1u8; ROUTING_INFO_SIZE];
        let mac_key = [2u8; INTEGRITY_MAC_KEY_SIZE];
        let hmac = routing::generate_routing_info_integrity_mac(mac_key, data);

        assert_eq!(true, check_integrity_mac(hmac, mac_key, data));
    }

    #[test]
    fn it_returns_false_if_mac_not_matching() {
        let data = [1u8; ROUTING_INFO_SIZE];
        let mac_key = [2u8; INTEGRITY_MAC_KEY_SIZE];
        let hmac = [0u8; INTEGRITY_MAC_SIZE];

        assert_eq!(false, check_integrity_mac(hmac, mac_key, data));
    }
}

#[cfg(test)]
mod check_zero_padding {
    use super::*;

    #[test]
    fn it_returns_a_correctly_padded_bytes() {
        let enc_header = [6u8; ROUTING_INFO_SIZE];
        let paddede_enc_header = add_zero_padding_to_encrypted_routing_information(&enc_header);
        assert_eq!(
            ROUTING_INFO_SIZE + 3 * SECURITY_PARAMETER,
            paddede_enc_header.len()
        );
    }
}

#[cfg(test)]
mod check_decryption {
    use super::*;
    use crate::header::crypto::STREAM_CIPHER_KEY_SIZE;
    use crate::header::routing::encrypt_routing_info;

    #[test]
    fn it_returns_output_equal_to_input_plaintext() {
        let routing_info = [9u8; ROUTING_INFO_SIZE];
        let key = [1u8; STREAM_CIPHER_KEY_SIZE];

        let enc_routing_info = encrypt_routing_info(key, &routing_info);
        let padded_enc_routing_info = [
            enc_routing_info.to_vec(),
            [0u8; 3 * SECURITY_PARAMETER].to_vec(),
        ]
        .concat();
        let decrypted_routing_info = decrypt_padded_routing_info(&key, &padded_enc_routing_info);
        assert_eq!(padded_enc_routing_info.len(), decrypted_routing_info.len());
        assert!(decrypted_routing_info[..routing_info.len()]
            .iter()
            .eq(routing_info.iter()));
    }
}
