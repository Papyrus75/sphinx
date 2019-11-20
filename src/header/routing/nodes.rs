use crate::constants::STREAM_CIPHER_OUTPUT_LENGTH;
use crate::header::keys::{HeaderIntegrityMacKey, StreamCipherKey};
use crate::header::mac::HeaderIntegrityMac;
use crate::header::routing::{
    EncapsulatedRoutingInformation, RoutingEncapsulationError, ENCRYPTED_ROUTING_INFO_SIZE,
    TRUNCATED_ROUTING_INFO_SIZE,
};
use crate::route::{NodeAddressBytes, RouteElement};
use crate::utils;
use crate::utils::crypto;
use crate::utils::crypto::STREAM_CIPHER_INIT_VECTOR;

// In paper beta
pub(super) struct RoutingInformation {
    node_address: NodeAddressBytes,
    // in paper nu
    header_integrity_mac: HeaderIntegrityMac,
    // in paper gamma
    next_routing_information: TruncatedRoutingInformation, // in paper also beta
}

impl RoutingInformation {
    pub(super) fn new(
        route_element: &RouteElement,
        next_encapsulated_routing_information: EncapsulatedRoutingInformation,
    ) -> Result<Self, RoutingEncapsulationError> {
        let node_address = match route_element {
            RouteElement::ForwardHop(mixnode) => mixnode.address,
            _ => return Err(RoutingEncapsulationError::IsNotForwardHopError),
        };

        Ok(RoutingInformation {
            node_address,
            header_integrity_mac: next_encapsulated_routing_information.integrity_mac,
            next_routing_information: next_encapsulated_routing_information
                .enc_routing_information
                .truncate(),
        })
    }

    fn concatenate_components(self) -> Vec<u8> {
        self.node_address
            .iter()
            .cloned()
            .chain(self.header_integrity_mac.get_value().iter().cloned())
            .chain(self.next_routing_information.iter().cloned())
            .collect()
    }

    pub(super) fn encrypt(self, key: StreamCipherKey) -> EncryptedRoutingInformation {
        let routing_info_components = self.concatenate_components();
        assert_eq!(ENCRYPTED_ROUTING_INFO_SIZE, routing_info_components.len());

        let pseudorandom_bytes = crypto::generate_pseudorandom_bytes(
            &key,
            &STREAM_CIPHER_INIT_VECTOR,
            STREAM_CIPHER_OUTPUT_LENGTH,
        );

        let encrypted_routing_info_vec = utils::bytes::xor(
            &routing_info_components,
            &pseudorandom_bytes[..ENCRYPTED_ROUTING_INFO_SIZE],
        );

        let mut encrypted_routing_info = [0u8; ENCRYPTED_ROUTING_INFO_SIZE];
        encrypted_routing_info.copy_from_slice(&encrypted_routing_info_vec);

        EncryptedRoutingInformation {
            value: encrypted_routing_info,
        }
    }
}

// result of xoring beta with rho (output of PRNG)
// the derivation is only required for the tests. please remove it in production
#[derive(Clone)]
pub struct EncryptedRoutingInformation {
    value: [u8; ENCRYPTED_ROUTING_INFO_SIZE],
}

impl EncryptedRoutingInformation {
    pub(super) fn from_bytes(bytes: [u8; ENCRYPTED_ROUTING_INFO_SIZE]) -> Self {
        Self { value: bytes }
    }

    fn truncate(self) -> TruncatedRoutingInformation {
        let mut truncated_routing_info = [0u8; TRUNCATED_ROUTING_INFO_SIZE];
        truncated_routing_info.copy_from_slice(&self.value[..TRUNCATED_ROUTING_INFO_SIZE]);
        truncated_routing_info
    }

    fn get_value(self) -> [u8; ENCRYPTED_ROUTING_INFO_SIZE] {
        self.value
    }

    pub fn get_value_ref(&self) -> &[u8] {
        self.value.as_ref()
    }

    pub(super) fn encapsulate_with_mac(
        self,
        key: HeaderIntegrityMacKey,
    ) -> EncapsulatedRoutingInformation {
        let integrity_mac = HeaderIntegrityMac::compute(key, &self.value);
        EncapsulatedRoutingInformation {
            enc_routing_information: self,
            integrity_mac,
        }
    }
}

// result of truncating encrypted beta before passing it to next 'layer'
type TruncatedRoutingInformation = [u8; TRUNCATED_ROUTING_INFO_SIZE];

#[cfg(test)]
mod preparing_header_layer {
    use crate::constants::HEADER_INTEGRITY_MAC_SIZE;
    use crate::header::keys::routing_keys_fixture;
    use crate::route::{node_address_fixture, MixNode};

    use super::*;
    use crate::header::routing::encapsulated_routing_information_fixture;

    #[test]
    fn it_returns_encrypted_truncated_address_concatenated_with_inner_layer_and_mac_on_it() {
        let address = node_address_fixture();
        let forward_hop = RouteElement::ForwardHop(MixNode {
            address,
            pub_key: Default::default(),
        });

        let routing_keys = routing_keys_fixture();
        let inner_layer_routing = encapsulated_routing_information_fixture();

        // calculate everything without using any object methods
        let concatenated_materials: Vec<u8> = [
            address.to_vec(),
            inner_layer_routing.integrity_mac.get_value_ref().to_vec(),
            inner_layer_routing
                .enc_routing_information
                .value
                .to_vec()
                .iter()
                .cloned()
                .take(TRUNCATED_ROUTING_INFO_SIZE)
                .collect(),
        ]
        .concat();

        let pseudorandom_bytes = crypto::generate_pseudorandom_bytes(
            &routing_keys.stream_cipher_key,
            &STREAM_CIPHER_INIT_VECTOR,
            STREAM_CIPHER_OUTPUT_LENGTH,
        );

        let expected_encrypted_routing_info_vec = utils::bytes::xor(
            &concatenated_materials,
            &pseudorandom_bytes[..ENCRYPTED_ROUTING_INFO_SIZE],
        );

        let mut expected_routing_mac = crypto::compute_keyed_hmac(
            routing_keys.header_integrity_hmac_key.to_vec(),
            &expected_encrypted_routing_info_vec,
        );
        expected_routing_mac.truncate(HEADER_INTEGRITY_MAC_SIZE);

        let next_layer_routing = RoutingInformation::new(&forward_hop, inner_layer_routing)
            .unwrap()
            .encrypt(routing_keys.stream_cipher_key)
            .encapsulate_with_mac(routing_keys.header_integrity_hmac_key);

        assert_eq!(
            expected_encrypted_routing_info_vec,
            next_layer_routing.enc_routing_information.value.to_vec()
        );
        assert_eq!(
            expected_routing_mac,
            next_layer_routing.integrity_mac.get_value()
        );
    }
}

#[cfg(test)]
mod encrypting_routing_information {
    use crate::header::mac::header_integrity_mac_fixture;
    use crate::route::node_address_fixture;
    use crate::utils::crypto::STREAM_CIPHER_KEY_SIZE;

    use super::*;

    #[test]
    fn it_is_possible_to_decrypt_it_to_recover_original_data() {
        let key = [2u8; STREAM_CIPHER_KEY_SIZE];
        let address = node_address_fixture();
        let mac = header_integrity_mac_fixture();
        let next_routing = [8u8; TRUNCATED_ROUTING_INFO_SIZE];

        let encryption_data = [
            address.to_vec(),
            mac.get_value_ref().to_vec(),
            next_routing.to_vec(),
        ]
        .concat();

        let routing_information = RoutingInformation {
            node_address: address,
            header_integrity_mac: mac,
            next_routing_information: next_routing,
        };

        let encrypted_data = routing_information.encrypt(key);
        let decryption_key_source = crypto::generate_pseudorandom_bytes(
            &key,
            &STREAM_CIPHER_INIT_VECTOR,
            STREAM_CIPHER_OUTPUT_LENGTH,
        );
        let decryption_key = &decryption_key_source[..ENCRYPTED_ROUTING_INFO_SIZE];
        let decrypted_data = utils::bytes::xor(&encrypted_data.value, decryption_key);
        assert_eq!(encryption_data, decrypted_data);
    }
}

#[cfg(test)]
mod truncating_routing_information {
    use super::*;

    #[test]
    fn it_does_not_change_prefixed_data() {
        let encrypted_routing_info = encrypted_routing_information_fixture();
        let routing_info_data_copy = encrypted_routing_info.value.clone();

        let truncated_routing_info = encrypted_routing_info.truncate();
        for i in 0..truncated_routing_info.len() {
            assert_eq!(truncated_routing_info[i], routing_info_data_copy[i]);
        }
    }
}

pub fn encrypted_routing_information_fixture() -> EncryptedRoutingInformation {
    EncryptedRoutingInformation {
        value: [5u8; ENCRYPTED_ROUTING_INFO_SIZE],
    }
}
