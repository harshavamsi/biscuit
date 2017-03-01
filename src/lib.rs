//! Create and parses JWT (JSON Web Tokens)
//!

#![cfg_attr(feature = "dev", allow(unstable_features))]
#![cfg_attr(feature = "dev", feature(plugin))]
#![cfg_attr(feature = "dev", plugin(clippy))]

// #![warn(missing_docs)]
#![doc(test(attr(allow(unused_variables), deny(warnings))))]

extern crate rustc_serialize;
extern crate ring;
extern crate untrusted;

use rustc_serialize::{json, Encodable, Decodable};
use rustc_serialize::base64::{self, ToBase64, FromBase64};
use rustc_serialize::json::{ToJson, Json};

#[cfg(test)]
#[macro_use]
mod test;
pub mod errors;
pub mod jws;

use errors::Error;
use std::collections::BTreeMap;

/// A part of the JWT: header and claims specifically
/// Allows converting from/to struct with base64
pub trait Part {
    type Encoded: AsRef<str>;

    fn from_base64<B: AsRef<[u8]>>(encoded: B) -> Result<Self, Error> where Self: Sized;
    fn to_base64(&self) -> Result<Self::Encoded, Error>;
}

impl<T> Part for T
    where T: Encodable + Decodable
{
    type Encoded = String;

    fn to_base64(&self) -> Result<Self::Encoded, Error> {
        let encoded = json::encode(&self)?;
        Ok(encoded.as_bytes().to_base64(base64::URL_SAFE))
    }

    fn from_base64<B: AsRef<[u8]>>(encoded: B) -> Result<T, Error> {
        let decoded = encoded.as_ref().from_base64()?;
        let s = String::from_utf8(decoded)?;
        Ok(json::decode(&s)?)
    }
}

#[derive(Debug, PartialEq, RustcDecodable)]
/// A basic JWT header part, the alg defaults to HS256 and typ is automatically
/// set to `JWT`. All the other fields are optional
pub struct Header {
    typ: String,
    pub alg: jws::Algorithm,
    pub jku: Option<String>,
    pub kid: Option<String>,
    pub x5u: Option<String>,
    pub x5t: Option<String>,
}

impl Header {
    pub fn new(algorithm: jws::Algorithm) -> Header {
        Header {
            typ: "JWT".to_string(),
            alg: algorithm,
            jku: None,
            kid: None,
            x5u: None,
            x5t: None,
        }
    }
}

impl Default for Header {
    fn default() -> Header {
        Header::new(jws::Algorithm::HS256)
    }
}

impl Encodable for Header {
    fn encode<S: rustc_serialize::Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        self.to_json().encode(s)
    }
}

impl ToJson for Header {
    fn to_json(&self) -> Json {
        let mut d = BTreeMap::new();
        d.insert("typ".to_string(), self.typ.to_json());
        d.insert("alg".to_string(), self.alg.to_json());

        // Define a macro to reduce boilerplate.
        macro_rules! optional {
            ($field_name:ident) => (
                if let Some(ref value) = self.$field_name {
                    d.insert(stringify!($field_name).to_string(), value.to_json());
                }
            )
        }
        optional!(jku);
        optional!(kid);
        optional!(x5u);
        optional!(x5t);
        Json::Object(d)
    }
}

#[derive(Debug)]
/// The return type of a successful call to decode(...)
pub struct TokenData<T: Part> {
    pub header: Header,
    pub claims: T,
}

/// Encode the claims passed and sign the payload using the algorithm from the header and the secret
pub fn encode<T: Part>(header: Header, claims: &T, secret: &[u8]) -> Result<String, Error> {
    let encoded_header = header.to_base64()?;
    let encoded_claims = claims.to_base64()?;
    // seems to be a tiny bit faster than format!("{}.{}", x, y)
    let payload = [encoded_header.as_ref(), encoded_claims.as_ref()].join(".");
    let signature = header.alg.sign(&*payload, secret.as_ref())?;

    Ok([payload, signature].join("."))
}

/// Used in decode: takes the result of a rsplit and ensure we only get 2 parts
/// Errors if we don't
macro_rules! expect_two {
    ($iter:expr) => {{
        let mut i = $iter; // evaluate the expr
        match (i.next(), i.next(), i.next()) {
            (Some(first), Some(second), None) => (first, second),
            _ => return Err(Error::InvalidToken)
        }
    }}
}

/// Decode a token into a Claims struct
/// If the token or its signature is invalid, it will return an error
pub fn decode<T: Part>(token: &str, secret: &[u8], algorithm: jws::Algorithm) -> Result<TokenData<T>, Error> {
    let (signature, payload) = expect_two!(token.rsplitn(2, '.'));

    let is_valid = algorithm.verify(signature, payload, secret);

    if !is_valid {
        return Err(Error::InvalidSignature);
    }

    let (claims, header) = expect_two!(payload.rsplitn(2, '.'));

    let header = Header::from_base64(header)?;
    if header.alg != algorithm {
        return Err(Error::WrongAlgorithmHeader);
    }
    let decoded_claims = T::from_base64(claims)?;

    Ok(TokenData {
        header: header,
        claims: decoded_claims,
    })
}

#[cfg(test)]
mod tests {
    use std::str;
    use super::{encode, decode, Header};
    use jws::Algorithm;

    #[derive(Debug, PartialEq, Clone, RustcEncodable, RustcDecodable)]
    struct Claims {
        sub: String,
        company: String,
    }

    #[test]
    fn encode_with_custom_header() {
        let expected_claims = Claims {
            sub: "b@b.com".to_string(),
            company: "ACME".to_string(),
        };
        let mut header = Header::default();
        header.kid = Some("kid".to_string());
        let token = not_err!(encode(header, &expected_claims, "secret".as_ref()));
        let token_data = not_err!(decode::<Claims>(&token, "secret".as_ref(), Algorithm::HS256));
        assert_eq!(expected_claims, token_data.claims);
        assert_eq!("kid", token_data.header.kid.unwrap());
    }

    #[test]
    fn round_trip_hs256() {
        let expected_token = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.\
                              eyJzdWIiOiJiQGIuY29tIiwiY29tcGFueSI6IkFDTUUifQ.\
                              C35LD5nqS_Gx9KF19E2wwf_KFcQ7TNqZLThivXZMXKWen9XVjr6kIF_fjZoaA-\
                              F9q1QjK4EAG6ZwFO2l3rL7MFsrOJwcCgfSkcnTLFOI_RewEFKSDDrfeZyXwQo4PlYd\
                              q5i2Ue1hxQwbv4MuVcnW1rEPqb04WMo3pS2IpNkJxbiUyWIz_Ze4enPXby8YRbidHfC0eS0CK\
                              7bvycE8RJC0Ynpdf0lnd_5jZmAQjC_imz9bjL_wLZq-ggl8Bbi-sA8VcIQWLTPbrpCuYPDrXkjdxL\
                              VpJXoBNEEkfNryqD9asu2r2tFJXrSVLxZGV9AAtkks7uk1nkyEfHVQiOE6JrNODA";
        let expected_claims = Claims {
            sub: "b@b.com".to_string(),
            company: "ACME".to_string(),
        };
        let private_key = ::test::read_private_key();

        let token = not_err!(encode(Header::new(Algorithm::RS256), &expected_claims, private_key));
        assert_eq!(expected_token, token);

        let token_data = not_err!(decode::<Claims>(&token, private_key, Algorithm::RS256));
        assert_eq!(expected_claims, token_data.claims);
        assert!(token_data.header.kid.is_none());
    }


    #[test]
    fn round_trip_rs256() {
        let expected_token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                              eyJzdWIiOiJiQGIuY29tIiwiY29tcGFueSI6IkFDTUUifQ.\
                              I1BvFoHe94AFf09O6tDbcSB8-jp8w6xZqmyHIwPeSdY";
        let expected_claims = Claims {
            sub: "b@b.com".to_string(),
            company: "ACME".to_string(),
        };

        let token = not_err!(encode(Header::new(Algorithm::HS256),
                                    &expected_claims,
                                    "secret".as_bytes()));
        assert_eq!(expected_token, token);

        let token_data = not_err!(decode::<Claims>(&token, "secret".as_bytes(), Algorithm::HS256));
        assert_eq!(expected_claims, token_data.claims);
        assert!(token_data.header.kid.is_none());
    }

    #[test]
    #[should_panic(expected = "InvalidToken")]
    fn decode_token_missing_parts() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let claims = decode::<Claims>(token, "secret".as_ref(), Algorithm::HS256);
        claims.unwrap();
    }

    #[test]
    #[should_panic(expected = "InvalidSignature")]
    fn decode_token_invalid_signature_hs256() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                     eyJzdWIiOiJiQGIuY29tIiwiY29tcGFueSI6IkFDTUUifQ.wrong";
        let claims = decode::<Claims>(token, "secret".as_ref(), Algorithm::HS256);
        claims.unwrap();
    }

    #[test]
    #[should_panic(expected = "InvalidSignature")]
    fn decode_token_invalid_signature_rs256() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                     eyJzdWIiOiJiQGIuY29tIiwiY29tcGFueSI6IkFDTUUifQ.wrong";
        let private_key = ::test::read_private_key();
        let claims = decode::<Claims>(token, private_key, Algorithm::RS256);
        claims.unwrap();
    }

    #[test]
    #[should_panic(expected = "WrongAlgorithmHeader")]
    fn decode_token_wrong_algorithm() {
        let token = "eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCJ9.\
                     eyJzdWIiOiJiQGIuY29tIiwiY29tcGFueSI6IkFDTUUifQ.\
                     pKscJVk7-aHxfmQKlaZxh5uhuKhGMAa-1F5IX5mfUwI";
        let claims = decode::<Claims>(token, "secret".as_ref(), Algorithm::HS256);
        claims.unwrap();
    }

    #[test]
    fn decode_token_with_bytes_secret_hs256() {
        let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.\
                     eyJzdWIiOiIxMjM0NTY3ODkwIiwiY29tcGFueSI6Ikdvb2dvbCJ9.\
                     27QxgG96vpX4akKNpD1YdRGHE3_u2X35wR3EHA2eCrs";
        let claims = decode::<Claims>(token, b"\x01\x02\x03", Algorithm::HS256);
        assert!(claims.is_ok());
    }

    #[test]
    fn decode_token_with_shuffled_header_fields() {
        let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.\
                     eyJjb21wYW55IjoiMTIzNDU2Nzg5MCIsInN1YiI6IkpvaG4gRG9lIn0.\
                     SEIZ4Jg46VGhquuwPYDLY5qHF8AkQczF14aXM3a2c28";
        let claims = decode::<Claims>(token, "secret".as_ref(), Algorithm::HS256);
        assert!(claims.is_ok());
    }
}
